//! Porte de `EditorParameterHints` (`editorOptions.ts`, linhas 3697 a 3756).

use crate::config::editor_options::option_support::{bool_input, object_input};
use crate::config::editor_options::{boolean, ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue};

/// Chave tipada de `parameterHints`.
pub const PARAMETER_HINTS: Opt<InternalParameterHintOptions> = Opt::new(EditorOption::ParameterHints);

/// `IEditorParameterHintOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IEditorParameterHintOptions {
    /// Liga as dicas de parâmetro. Padrão `true`.
    pub enabled: Option<bool>,
    /// Liga a navegação cíclica entre as dicas.
    pub cycle: Option<bool>,
}

/// `InternalParameterHintOptions` (`Readonly<Required<IEditorParameterHintOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalParameterHintOptions {
    pub enabled: bool,
    pub cycle: bool,
}

/// `EditorParameterHints`.
pub struct EditorParameterHints {
    default_value: InternalParameterHintOptions,
    schema: OptionSchema,
}

impl EditorParameterHints {
    pub fn new() -> Self {
        // O padrão de `cycle` é `true` no original, apesar do comentário da interface dizer `false`.
        let defaults = InternalParameterHintOptions { enabled: true, cycle: true };
        let schema = OptionSchema::Map(vec![
            (
                "editor.parameterHints.enabled".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("boolean".to_string()),
                    default: Some(SchemaValue::Bool(defaults.enabled)),
                    description: Some("Enables a pop-up that shows parameter documentation and type information as you type.".to_string()),
                    ..Default::default()
                },
            ),
            (
                "editor.parameterHints.cycle".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("boolean".to_string()),
                    default: Some(SchemaValue::Bool(defaults.cycle)),
                    description: Some("Controls whether the parameter hints menu cycles or closes when reaching the end of the list.".to_string()),
                    ..Default::default()
                },
            ),
        ]);
        Self { default_value: defaults, schema }
    }

    /// `validate` com a interface já montada; `None` é o caso `!_input || typeof _input !== 'object'`.
    pub fn validate_options(&self, input: Option<&IEditorParameterHintOptions>) -> InternalParameterHintOptions {
        let Some(input) = input else {
            return self.default_value;
        };
        InternalParameterHintOptions {
            enabled: boolean(&bool_input(input.enabled), self.default_value.enabled),
            cycle: boolean(&bool_input(input.cycle), self.default_value.cycle),
        }
    }
}

impl Default for EditorParameterHints {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorParameterHints {
    type Value = InternalParameterHintOptions;

    fn id(&self) -> EditorOption {
        EditorOption::ParameterHints
    }

    fn name(&self) -> &str {
        "parameterHints"
    }

    fn default_value(&self) -> InternalParameterHintOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> InternalParameterHintOptions {
        self.validate_options(object_input::<IEditorParameterHintOptions>(input).as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_flags_default_to_true() {
        let value = EditorParameterHints::new().default_value();
        assert!(value.enabled && value.cycle);
    }

    #[test]
    fn non_objects_and_empty_objects_give_the_default() {
        let option = EditorParameterHints::new();
        assert_eq!(option.validate_options(None), option.default_value());
        for input in [OptionInput::Undefined, OptionInput::Null, OptionInput::Bool(false), OptionInput::Number(0.0), OptionInput::string("false")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate(&OptionInput::Other), option.default_value());
    }

    #[test]
    fn flags_are_independent() {
        let option = EditorParameterHints::new();
        let value = option.validate_options(Some(&IEditorParameterHintOptions { enabled: Some(false), cycle: None }));
        assert_eq!(value, InternalParameterHintOptions { enabled: false, cycle: true });
        let value = option.validate_options(Some(&IEditorParameterHintOptions { enabled: None, cycle: Some(false) }));
        assert_eq!(value, InternalParameterHintOptions { enabled: true, cycle: false });
        let value = option.validate_options(Some(&IEditorParameterHintOptions { enabled: Some(false), cycle: Some(false) }));
        assert_eq!(value, InternalParameterHintOptions { enabled: false, cycle: false });
    }

    #[test]
    fn schema_defaults_match_the_default_value() {
        let option = EditorParameterHints::new();
        let Some(OptionSchema::Map(entries)) = option.schema() else { panic!("sem esquema") };
        assert_eq!(entries[0].0, "editor.parameterHints.enabled");
        assert_eq!(entries[1].0, "editor.parameterHints.cycle");
        assert_eq!(entries[1].1.default, Some(SchemaValue::Bool(true)));
    }
}
