//! Porte de `EditorPadding` (`editorOptions.ts`, linhas 3640 a 3695).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{clamped_int, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `padding`.
pub const PADDING: Opt<InternalEditorPaddingOptions> = Opt::new(EditorOption::Padding);

/// `InternalEditorPaddingOptions` (`Readonly<Required<IEditorPaddingOptions>>`): `top` é o espaço entre a
/// borda superior do editor e a primeira linha, `bottom` entre a borda inferior e a última.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InternalEditorPaddingOptions {
    pub top: f64,
    pub bottom: f64,
}

/// `EditorPadding`.
pub struct EditorPadding {
    schema: Dynamic,
}

impl EditorPadding {
    pub fn new() -> Self {
        let side = |description: &str| {
            Dynamic::object([
                ("type", Dynamic::string("number")),
                ("default", Dynamic::Number(0.0)),
                ("minimum", Dynamic::Number(0.0)),
                ("maximum", Dynamic::Number(1000.0)),
                ("description", Dynamic::string(description)),
            ])
        };
        let schema = Dynamic::object([
            ("editor.padding.top", side("Controls the amount of space between the top edge of the editor and the first line.")),
            ("editor.padding.bottom", side("Controls the amount of space between the bottom edge of the editor and the last line.")),
        ]);
        Self { schema }
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

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> InternalEditorPaddingOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value();
        }
        InternalEditorPaddingOptions {
            top: clamped_int(&input.get_str("top"), 0.0, 0.0, 1000.0),
            bottom: clamped_int(&input.get_str("bottom"), 0.0, 0.0, 1000.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn padding(top: Dynamic, bottom: Dynamic) -> InternalEditorPaddingOptions {
        EditorPadding::new().validate(&Dynamic::object([("top", top), ("bottom", bottom)]))
    }

    fn num(n: f64) -> Dynamic {
        Dynamic::Number(n)
    }

    #[test]
    fn default_is_zero_on_both_sides() {
        assert_eq!(EditorPadding::new().default_value(), InternalEditorPaddingOptions { top: 0.0, bottom: 0.0 });
    }

    #[test]
    fn non_objects_and_empty_objects_give_the_default() {
        let option = EditorPadding::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), num(5.0), Dynamic::string("5")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate(&Dynamic::object::<&str>([])), option.default_value());
    }

    #[test]
    fn sides_are_clamped_integers() {
        assert_eq!(padding(num(10.0), num(20.0)), InternalEditorPaddingOptions { top: 10.0, bottom: 20.0 });
        assert_eq!(padding(num(-5.0), num(5000.0)), InternalEditorPaddingOptions { top: 0.0, bottom: 1000.0 });
        assert_eq!(padding(num(7.9), Dynamic::Undefined), InternalEditorPaddingOptions { top: 7.0, bottom: 0.0 });
        assert_eq!(padding(num(f64::NAN), num(3.0)), InternalEditorPaddingOptions { top: 0.0, bottom: 3.0 });
        assert_eq!(padding(Dynamic::string("12px"), Dynamic::string("x")), InternalEditorPaddingOptions { top: 12.0, bottom: 0.0 });
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = EditorPadding::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["editor.padding.top", "editor.padding.bottom"]);
        let top = schema.get_str("editor.padding.top");
        assert_eq!(top.own_keys(), vec!["type", "default", "minimum", "maximum", "description"]);
        assert_eq!((top.get_str("minimum"), top.get_str("maximum")), (num(0.0), num(1000.0)));
    }
}
