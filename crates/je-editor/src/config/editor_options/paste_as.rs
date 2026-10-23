//! Porte de `EditorPasteAs`, `IPasteAsOptions` e `EditorPasteAsOptions` (`editorOptions.ts`, linhas 5769
//! a 5834).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, strings_array, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `pasteAs`.
pub const PASTE_AS: Opt<EditorPasteAsOptions> = Opt::new(EditorOption::PasteAs);

string_enum! {
    /// `showPasteSelector: 'afterPaste' | 'never'`.
    pub enum ShowPasteSelector {
        AfterPaste => "afterPaste",
        Never => "never",
    }
}

/// `EditorPasteAsOptions` (`Readonly<Required<IPasteAsOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorPasteAsOptions {
    /// Liga o "colar como" nos editores.
    pub enabled: bool,
    /// Se um widget aparece depois de colar.
    pub show_paste_selector: ShowPasteSelector,
}

/// `EditorPasteAs`.
pub struct EditorPasteAs {
    default_value: EditorPasteAsOptions,
    schema: Dynamic,
}

impl EditorPasteAs {
    pub fn new() -> Self {
        let defaults = EditorPasteAsOptions { enabled: true, show_paste_selector: ShowPasteSelector::AfterPaste };
        let schema = Dynamic::object([
            (
                "editor.pasteAs.enabled",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", Dynamic::Bool(defaults.enabled)),
                    ("markdownDescription", Dynamic::string("Controls whether you can paste content in different ways.")),
                ]),
            ),
            (
                "editor.pasteAs.showPasteSelector",
                Dynamic::object([
                    ("type", Dynamic::string("string")),
                    (
                        "markdownDescription",
                        Dynamic::string("Controls if a widget is shown when pasting content in to the editor. This widget lets you control how the file is pasted."),
                    ),
                    ("enum", strings_array(ShowPasteSelector::ALLOWED)),
                    (
                        "enumDescriptions",
                        strings_array(&[
                            "Show the paste selector widget after content is pasted into the editor.",
                            "Never show the paste selector widget. Instead the default pasting behavior is always used.",
                        ]),
                    ),
                    ("default", Dynamic::string("afterPaste")),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorPasteAs {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorPasteAs {
    type Value = EditorPasteAsOptions;

    fn id(&self) -> EditorOption {
        EditorOption::PasteAs
    }

    fn name(&self) -> &str {
        "pasteAs"
    }

    fn default_value(&self) -> EditorPasteAsOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorPasteAsOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        EditorPasteAsOptions {
            enabled: boolean(&input.get_str("enabled"), self.default_value.enabled),
            show_paste_selector: ShowPasteSelector::validate(&input.get_str("showPasteSelector"), self.default_value.show_paste_selector),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> EditorPasteAsOptions {
        EditorPasteAs::new().validate(&Dynamic::object(entries))
    }

    #[test]
    fn invalid_inputs_and_empty_objects_give_the_default() {
        let option = EditorPasteAs::new();
        let defaults = EditorPasteAsOptions { enabled: true, show_paste_selector: ShowPasteSelector::AfterPaste };
        assert_eq!(option.default_value(), defaults);
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(1.0), text("never")] {
            assert_eq!(option.validate(&input), defaults);
        }
        assert_eq!(validate(vec![]), defaults);
    }

    #[test]
    fn fields_are_validated_individually() {
        let value = validate(vec![("enabled", Dynamic::Bool(false)), ("showPasteSelector", text("never"))]);
        assert_eq!(value, EditorPasteAsOptions { enabled: false, show_paste_selector: ShowPasteSelector::Never });
        let value = validate(vec![("enabled", text("false")), ("showPasteSelector", text("afterDrop"))]);
        assert_eq!(value, EditorPasteAsOptions { enabled: false, show_paste_selector: ShowPasteSelector::AfterPaste });
        assert_eq!(validate(vec![("showPasteSelector", Dynamic::Null)]).show_paste_selector, ShowPasteSelector::AfterPaste);
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = EditorPasteAs::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["editor.pasteAs.enabled", "editor.pasteAs.showPasteSelector"]);
        assert_eq!(schema.get_str("editor.pasteAs.enabled").own_keys(), vec!["type", "default", "markdownDescription"]);
        let selector = schema.get_str("editor.pasteAs.showPasteSelector");
        assert_eq!(selector.own_keys(), vec!["type", "markdownDescription", "enum", "enumDescriptions", "default"]);
        assert_eq!(selector.get_str("default"), text("afterPaste"));
        assert_eq!(selector.get_str("enum").get_str("0"), text("afterPaste"));
    }
}
