//! Porte de `SmartSelect`, `ISmartSelectOptions` e `SmartSelectOptions` (`editorOptions.ts`, linhas 5490
//! a 5535).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `smartSelect`.
pub const SMART_SELECT: Opt<SmartSelectOptions> = Opt::new(EditorOption::SmartSelect);

/// `SmartSelectOptions` (`Readonly<Required<ISmartSelectOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartSelectOptions {
    /// Seleciona sempre o espaço em branco do começo e do fim.
    pub select_leading_and_trailing_whitespace: bool,
    /// Seleciona subpalavras (`foo` em `fooBar` ou `foo_bar`).
    pub select_subwords: bool,
}

/// `{ description, default: true, type: 'boolean' }`.
fn entry(description: &str) -> Dynamic {
    Dynamic::object([
        ("description", Dynamic::string(description)),
        ("default", Dynamic::Bool(true)),
        ("type", Dynamic::string("boolean")),
    ])
}

/// `SmartSelect`.
pub struct SmartSelect {
    default_value: SmartSelectOptions,
    schema: Dynamic,
}

impl SmartSelect {
    pub fn new() -> Self {
        let schema = Dynamic::object([
            (
                "editor.smartSelect.selectLeadingAndTrailingWhitespace",
                entry("Whether leading and trailing whitespace should always be selected."),
            ),
            (
                "editor.smartSelect.selectSubwords",
                entry("Whether subwords (like 'foo' in 'fooBar' or 'foo_bar') should be selected."),
            ),
        ]);
        Self {
            default_value: SmartSelectOptions { select_leading_and_trailing_whitespace: true, select_subwords: true },
            schema,
        }
    }
}

impl Default for SmartSelect {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for SmartSelect {
    type Value = SmartSelectOptions;

    fn id(&self) -> EditorOption {
        EditorOption::SmartSelect
    }

    fn name(&self) -> &str {
        "smartSelect"
    }

    fn default_value(&self) -> SmartSelectOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> SmartSelectOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        SmartSelectOptions {
            select_leading_and_trailing_whitespace: boolean(
                &input.get_str("selectLeadingAndTrailingWhitespace"),
                self.default_value.select_leading_and_trailing_whitespace,
            ),
            select_subwords: boolean(&input.get_str("selectSubwords"), self.default_value.select_subwords),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(entries: Vec<(&str, Dynamic)>) -> SmartSelectOptions {
        SmartSelect::new().validate(&Dynamic::object(entries))
    }

    #[test]
    fn invalid_inputs_and_empty_objects_give_the_default() {
        let option = SmartSelect::new();
        let defaults = SmartSelectOptions { select_leading_and_trailing_whitespace: true, select_subwords: true };
        assert_eq!(option.default_value(), defaults);
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(false), Dynamic::Number(1.0), Dynamic::string("x")] {
            assert_eq!(option.validate(&input), defaults);
        }
        assert_eq!(validate(vec![]), defaults);
    }

    #[test]
    fn fields_follow_the_boolean_helper() {
        let value = validate(vec![
            ("selectLeadingAndTrailingWhitespace", Dynamic::string("false")),
            ("selectSubwords", Dynamic::Bool(false)),
        ]);
        assert!(!value.select_leading_and_trailing_whitespace && !value.select_subwords);
        let value = validate(vec![("selectSubwords", Dynamic::Number(0.0))]);
        assert!(value.select_leading_and_trailing_whitespace && !value.select_subwords);
        let value = validate(vec![("selectSubwords", Dynamic::Null)]);
        assert!(!value.select_subwords);
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = SmartSelect::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec!["editor.smartSelect.selectLeadingAndTrailingWhitespace", "editor.smartSelect.selectSubwords"]
        );
        let subwords = schema.get_str("editor.smartSelect.selectSubwords");
        assert_eq!(subwords.own_keys(), vec!["description", "default", "type"]);
        assert_eq!(subwords.get_str("default"), Dynamic::Bool(true));
        assert_eq!(subwords.get_str("type"), Dynamic::string("boolean"));
    }
}
