//! Porte de `EditorEmptySelectionClipboard` (`editorOptions.ts`, região `emptySelectionClipboard`): uma
//! `EditorBooleanOption` cujo valor calculado também depende do ambiente.

use crate::config::editor_options::{
    ComputedEditorOptions, ConfigurationPropertySchema, EditorBooleanOption, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt,
    OptionInput, OptionSchema,
};

/// Chave tipada da opção `emptySelectionClipboard`.
pub const EMPTY_SELECTION_CLIPBOARD: Opt<bool> = Opt::new(EditorOption::EmptySelectionClipboard);

/// `EditorEmptySelectionClipboard`: a [`EditorBooleanOption`] do original com o `compute` sobrescrito.
pub struct EditorEmptySelectionClipboard {
    base: EditorBooleanOption,
}

impl EditorEmptySelectionClipboard {
    pub fn new() -> Self {
        Self {
            base: EditorBooleanOption::new(
                EditorOption::EmptySelectionClipboard,
                "emptySelectionClipboard",
                true,
                Some(ConfigurationPropertySchema {
                    description: Some("Controls whether copying without a selection copies the current line.".to_string()),
                    ..Default::default()
                }),
            ),
        }
    }
}

impl Default for EditorEmptySelectionClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorEmptySelectionClipboard {
    type Value = bool;

    fn id(&self) -> EditorOption {
        EditorOptionImpl::id(&self.base)
    }

    fn name(&self) -> &str {
        EditorOptionImpl::name(&self.base)
    }

    fn default_value(&self) -> bool {
        EditorOptionImpl::default_value(&self.base)
    }

    fn schema(&self) -> Option<&OptionSchema> {
        EditorOptionImpl::schema(&self.base)
    }

    fn validate(&self, input: &OptionInput) -> bool {
        EditorOptionImpl::validate(&self.base, input)
    }

    fn compute(&self, env: &EnvironmentalOptions, _options: &ComputedEditorOptions, value: &bool) -> bool {
        *value && env.empty_selection_clipboard
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing;
    use crate::config::editor_options::SchemaValue;

    #[test]
    fn validate_is_the_boolean_option_validate() {
        let option = EditorEmptySelectionClipboard::new();
        assert!(EditorOptionImpl::default_value(&option));
        assert!(EditorOptionImpl::validate(&option, &OptionInput::Undefined));
        assert!(!EditorOptionImpl::validate(&option, &OptionInput::string("false")));
        assert!(!EditorOptionImpl::validate(&option, &OptionInput::Number(0.0)));
        assert!(EditorOptionImpl::validate(&option, &OptionInput::string("x")));
        assert_eq!(EditorOptionImpl::name(&option), "emptySelectionClipboard");
    }

    #[test]
    fn compute_combines_the_value_with_the_environment() {
        let option = EditorEmptySelectionClipboard::new();
        let options = testing::options_with(vec![]);
        let mut env = testing::environment(testing::font_info(true, 14.0, "normal"));
        env.empty_selection_clipboard = true;
        assert!(EditorOptionImpl::compute(&option, &env, &options, &true));
        assert!(!EditorOptionImpl::compute(&option, &env, &options, &false));
        env.empty_selection_clipboard = false;
        assert!(!EditorOptionImpl::compute(&option, &env, &options, &true));
        assert!(!EditorOptionImpl::compute(&option, &env, &options, &false));
    }

    #[test]
    fn schema_is_a_typed_boolean() {
        let option = EditorEmptySelectionClipboard::new();
        let Some(OptionSchema::Single(schema)) = EditorOptionImpl::schema(&option) else { panic!("sem esquema") };
        assert_eq!(schema.type_.as_deref(), Some("boolean"));
        assert_eq!(schema.default, Some(SchemaValue::Bool(true)));
    }
}
