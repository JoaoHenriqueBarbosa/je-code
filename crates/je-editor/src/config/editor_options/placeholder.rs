//! Porte de `PlaceholderOption` (`editorOptions.ts`, linhas 3773 a 3790).

use crate::config::editor_options::{EditorOption, EditorOptionImpl, Opt, OptionInput};

/// Chave tipada de `placeholder`.
pub const PLACEHOLDER: Opt<Option<String>> = Opt::new(EditorOption::Placeholder);

/// `PlaceholderOption`: `string | undefined`, sem esquema.
pub struct PlaceholderOption;

impl PlaceholderOption {
    pub fn new() -> Self {
        PlaceholderOption
    }
}

impl Default for PlaceholderOption {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for PlaceholderOption {
    type Value = Option<String>;

    fn id(&self) -> EditorOption {
        EditorOption::Placeholder
    }

    fn name(&self) -> &str {
        "placeholder"
    }

    fn default_value(&self) -> Option<String> {
        None
    }

    fn validate(&self, input: &OptionInput) -> Option<String> {
        match input {
            OptionInput::String(text) => Some(text.clone()),
            // `undefined` e qualquer outro tipo dão o padrão.
            _ => self.default_value(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_pass_through() {
        let option = PlaceholderOption::new();
        assert_eq!(option.validate(&OptionInput::string("Digite aqui")), Some("Digite aqui".to_string()));
        assert_eq!(option.validate(&OptionInput::string("")), Some(String::new()));
    }

    #[test]
    fn everything_else_is_undefined() {
        let option = PlaceholderOption::new();
        for input in [OptionInput::Undefined, OptionInput::Null, OptionInput::Bool(true), OptionInput::Number(1.0), OptionInput::Other] {
            assert_eq!(option.validate(&input), None);
        }
    }

    #[test]
    fn identity() {
        let option = PlaceholderOption::new();
        assert_eq!(option.id(), EditorOption::Placeholder);
        assert_eq!(option.name(), "placeholder");
        assert_eq!(option.default_value(), None);
        assert!(option.schema().is_none());
    }
}
