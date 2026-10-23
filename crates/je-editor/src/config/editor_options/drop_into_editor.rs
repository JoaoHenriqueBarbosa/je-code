//! Porte de `EditorDropIntoEditor`, `IDropIntoEditorOptions` e `EditorDropIntoEditorOptions`
//! (`editorOptions.ts`, linhas 5702 a 5767).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, strings_array, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `dropIntoEditor`.
pub const DROP_INTO_EDITOR: Opt<EditorDropIntoEditorOptions> = Opt::new(EditorOption::DropIntoEditor);

string_enum! {
    /// `showDropSelector: 'afterDrop' | 'never'`.
    pub enum ShowDropSelector {
        AfterDrop => "afterDrop",
        Never => "never",
    }
}

/// `EditorDropIntoEditorOptions` (`Readonly<Required<IDropIntoEditorOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorDropIntoEditorOptions {
    /// Permite soltar um arquivo no editor.
    pub enabled: bool,
    /// Se um widget aparece depois de soltar.
    pub show_drop_selector: ShowDropSelector,
}

/// `EditorDropIntoEditor`.
pub struct EditorDropIntoEditor {
    default_value: EditorDropIntoEditorOptions,
    schema: Dynamic,
}

impl EditorDropIntoEditor {
    pub fn new() -> Self {
        let defaults = EditorDropIntoEditorOptions { enabled: true, show_drop_selector: ShowDropSelector::AfterDrop };
        let schema = Dynamic::object([
            (
                "editor.dropIntoEditor.enabled",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", Dynamic::Bool(defaults.enabled)),
                    (
                        "markdownDescription",
                        Dynamic::string("Controls whether you can drag and drop a file into a text editor by holding down the `Shift` key (instead of opening the file in an editor)."),
                    ),
                ]),
            ),
            (
                "editor.dropIntoEditor.showDropSelector",
                Dynamic::object([
                    ("type", Dynamic::string("string")),
                    (
                        "markdownDescription",
                        Dynamic::string("Controls if a widget is shown when dropping files into the editor. This widget lets you control how the file is dropped."),
                    ),
                    ("enum", strings_array(ShowDropSelector::ALLOWED)),
                    (
                        "enumDescriptions",
                        strings_array(&[
                            "Show the drop selector widget after a file is dropped into the editor.",
                            "Never show the drop selector widget. Instead the default drop provider is always used.",
                        ]),
                    ),
                    ("default", Dynamic::string("afterDrop")),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorDropIntoEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorDropIntoEditor {
    type Value = EditorDropIntoEditorOptions;

    fn id(&self) -> EditorOption {
        EditorOption::DropIntoEditor
    }

    fn name(&self) -> &str {
        "dropIntoEditor"
    }

    fn default_value(&self) -> EditorDropIntoEditorOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorDropIntoEditorOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        EditorDropIntoEditorOptions {
            enabled: boolean(&input.get_str("enabled"), self.default_value.enabled),
            show_drop_selector: ShowDropSelector::validate(&input.get_str("showDropSelector"), self.default_value.show_drop_selector),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> EditorDropIntoEditorOptions {
        EditorDropIntoEditor::new().validate(&Dynamic::object(entries))
    }

    #[test]
    fn invalid_inputs_and_empty_objects_give_the_default() {
        let option = EditorDropIntoEditor::new();
        let defaults = EditorDropIntoEditorOptions { enabled: true, show_drop_selector: ShowDropSelector::AfterDrop };
        assert_eq!(option.default_value(), defaults);
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(1.0), text("never")] {
            assert_eq!(option.validate(&input), defaults);
        }
        assert_eq!(validate(vec![]), defaults);
    }

    #[test]
    fn fields_are_validated_individually() {
        let value = validate(vec![("enabled", Dynamic::Bool(false)), ("showDropSelector", text("never"))]);
        assert_eq!(value, EditorDropIntoEditorOptions { enabled: false, show_drop_selector: ShowDropSelector::Never });
        let value = validate(vec![("enabled", text("false")), ("showDropSelector", text("always"))]);
        assert_eq!(value, EditorDropIntoEditorOptions { enabled: false, show_drop_selector: ShowDropSelector::AfterDrop });
        assert_eq!(validate(vec![("showDropSelector", Dynamic::Number(1.0))]).show_drop_selector, ShowDropSelector::AfterDrop);
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = EditorDropIntoEditor::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["editor.dropIntoEditor.enabled", "editor.dropIntoEditor.showDropSelector"]);
        assert_eq!(schema.get_str("editor.dropIntoEditor.enabled").own_keys(), vec!["type", "default", "markdownDescription"]);
        let selector = schema.get_str("editor.dropIntoEditor.showDropSelector");
        assert_eq!(selector.own_keys(), vec!["type", "markdownDescription", "enum", "enumDescriptions", "default"]);
        assert_eq!(selector.get_str("default"), text("afterDrop"));
        assert_eq!(selector.get_str("enum").get_str("1"), text("never"));
    }
}
