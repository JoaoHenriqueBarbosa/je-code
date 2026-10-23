//! Porte de `EditorPadding` (`editorOptions.ts`, linhas 3640 a 3695).

use crate::config::editor_options::option_support::{number_input, object_input};
use crate::config::editor_options::{clamped_int, ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue};

/// Chave tipada de `padding`.
pub const PADDING: Opt<InternalEditorPaddingOptions> = Opt::new(EditorOption::Padding);

/// `IEditorPaddingOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IEditorPaddingOptions {
    /// Espaço entre a borda superior do editor e a primeira linha.
    pub top: Option<f64>,
    /// Espaço entre a borda inferior do editor e a última linha.
    pub bottom: Option<f64>,
}

/// `InternalEditorPaddingOptions` (`Readonly<Required<IEditorPaddingOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InternalEditorPaddingOptions {
    pub top: f64,
    pub bottom: f64,
}

/// `EditorPadding`.
pub struct EditorPadding {
    schema: OptionSchema,
}

impl EditorPadding {
    pub fn new() -> Self {
        let side = |description: &str| ConfigurationPropertySchema {
            type_: Some("number".to_string()),
            default: Some(SchemaValue::Number(0.0)),
            minimum: Some(0.0),
            maximum: Some(1000.0),
            description: Some(description.to_string()),
            ..Default::default()
        };
        let schema = OptionSchema::Map(vec![
            ("editor.padding.top".to_string(), side("Controls the amount of space between the top edge of the editor and the first line.")),
            ("editor.padding.bottom".to_string(), side("Controls the amount of space between the bottom edge of the editor and the last line.")),
        ]);
        Self { schema }
    }

    /// `validate` com a interface já montada; `None` é o caso `!_input || typeof _input !== 'object'`.
    pub fn validate_options(&self, input: Option<&IEditorPaddingOptions>) -> InternalEditorPaddingOptions {
        let Some(input) = input else {
            return self.default_value();
        };
        InternalEditorPaddingOptions {
            top: clamped_int(&number_input(input.top), 0.0, 0.0, 1000.0),
            bottom: clamped_int(&number_input(input.bottom), 0.0, 0.0, 1000.0),
        }
    }
}

impl Default for EditorPadding {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorPadding {
    type Value = InternalEditorPaddingOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Padding
    }

    fn name(&self) -> &str {
        "padding"
    }

    fn default_value(&self) -> InternalEditorPaddingOptions {
        InternalEditorPaddingOptions { top: 0.0, bottom: 0.0 }
    }

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> InternalEditorPaddingOptions {
        self.validate_options(object_input::<IEditorPaddingOptions>(input).as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn padding(top: Option<f64>, bottom: Option<f64>) -> InternalEditorPaddingOptions {
        EditorPadding::new().validate_options(Some(&IEditorPaddingOptions { top, bottom }))
    }

    #[test]
    fn default_is_zero_on_both_sides() {
        assert_eq!(EditorPadding::new().default_value(), InternalEditorPaddingOptions { top: 0.0, bottom: 0.0 });
    }

    #[test]
    fn non_objects_and_empty_objects_give_the_default() {
        let option = EditorPadding::new();
        assert_eq!(option.validate_options(None), option.default_value());
        for input in [OptionInput::Undefined, OptionInput::Null, OptionInput::Bool(true), OptionInput::Number(5.0), OptionInput::string("5")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate(&OptionInput::Other), option.default_value());
    }

    #[test]
    fn sides_are_clamped_integers() {
        assert_eq!(padding(Some(10.0), Some(20.0)), InternalEditorPaddingOptions { top: 10.0, bottom: 20.0 });
        assert_eq!(padding(Some(-5.0), Some(5000.0)), InternalEditorPaddingOptions { top: 0.0, bottom: 1000.0 });
        assert_eq!(padding(Some(7.9), None), InternalEditorPaddingOptions { top: 7.0, bottom: 0.0 });
        assert_eq!(padding(Some(f64::NAN), Some(3.0)), InternalEditorPaddingOptions { top: 0.0, bottom: 3.0 });
    }

    #[test]
    fn schema_has_top_and_bottom() {
        let option = EditorPadding::new();
        let Some(OptionSchema::Map(entries)) = option.schema() else { panic!("sem esquema") };
        assert_eq!(entries[0].0, "editor.padding.top");
        assert_eq!(entries[1].0, "editor.padding.bottom");
        assert_eq!((entries[0].1.minimum, entries[0].1.maximum), (Some(0.0), Some(1000.0)));
    }
}
