//! Porte de `EditorLightbulb` e `ShowLightbulbIconMode` (`editorOptions.ts`, linhas 3109 a 3169).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{strings_array, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `lightbulb`.
pub const LIGHTBULB: Opt<EditorLightbulbOptions> = Opt::new(EditorOption::Lightbulb);

string_enum! {
    /// `ShowLightbulbIconMode`.
    pub enum ShowLightbulbIconMode {
        Off => "off",
        OnCode => "onCode",
        On => "on",
    }
}

/// `EditorLightbulbOptions` (`Readonly<Required<IEditorLightbulbOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorLightbulbOptions {
    pub enabled: ShowLightbulbIconMode,
}

/// `EditorLightbulb`.
pub struct EditorLightbulb {
    default_value: EditorLightbulbOptions,
    schema: Dynamic,
}

impl EditorLightbulb {
    pub fn new() -> Self {
        let defaults = EditorLightbulbOptions { enabled: ShowLightbulbIconMode::OnCode };
        let property = Dynamic::object([
            ("type", Dynamic::string("string")),
            ("enum", strings_array(ShowLightbulbIconMode::ALLOWED)),
            ("default", Dynamic::string(defaults.enabled.as_str())),
            (
                "enumDescriptions",
                Dynamic::array([
                    Dynamic::string("Disable the code action menu."),
                    Dynamic::string("Show the code action menu when the cursor is on lines with code."),
                    Dynamic::string("Show the code action menu when the cursor is on lines with code or on empty lines."),
                ]),
            ),
            ("description", Dynamic::string("Enables the Code Action lightbulb in the editor.")),
        ]);
        Self { default_value: defaults, schema: Dynamic::object([("editor.lightbulb.enabled", property)]) }
    }
}

impl Default for EditorLightbulb {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorLightbulb {
    type Value = EditorLightbulbOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Lightbulb
    }

    fn name(&self) -> &str {
        "lightbulb"
    }

    fn default_value(&self) -> EditorLightbulbOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorLightbulbOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        EditorLightbulbOptions { enabled: ShowLightbulbIconMode::validate(&input.get_str("enabled"), self.default_value.enabled) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(enabled: Dynamic) -> Dynamic {
        Dynamic::object([("enabled", enabled)])
    }

    #[test]
    fn default_is_on_code() {
        assert_eq!(EditorLightbulb::new().default_value().enabled, ShowLightbulbIconMode::OnCode);
    }

    #[test]
    fn non_objects_give_the_default() {
        let option = EditorLightbulb::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(1.0), Dynamic::string("on"), Dynamic::string("")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
    }

    #[test]
    fn empty_object_and_array_give_the_default() {
        let option = EditorLightbulb::new();
        assert_eq!(option.validate(&Dynamic::object::<&str>([])), option.default_value());
        // Um array também é `typeof 'object'`: `input.enabled` é `undefined`.
        assert_eq!(option.validate(&Dynamic::array([])), option.default_value());
    }

    #[test]
    fn enabled_accepts_only_the_three_modes() {
        let option = EditorLightbulb::new();
        let enabled = |value: Dynamic| option.validate(&with(value)).enabled;
        assert_eq!(enabled(Dynamic::string("off")), ShowLightbulbIconMode::Off);
        assert_eq!(enabled(Dynamic::string("on")), ShowLightbulbIconMode::On);
        assert_eq!(enabled(Dynamic::string("onCode")), ShowLightbulbIconMode::OnCode);
        assert_eq!(enabled(Dynamic::string("oncode")), ShowLightbulbIconMode::OnCode);
        assert_eq!(enabled(Dynamic::Bool(false)), ShowLightbulbIconMode::OnCode);
        assert_eq!(enabled(Dynamic::Undefined), ShowLightbulbIconMode::OnCode);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = EditorLightbulb::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["editor.lightbulb.enabled"]);
        let property = schema.get_str("editor.lightbulb.enabled");
        assert_eq!(property.own_keys(), vec!["type", "enum", "default", "enumDescriptions", "description"]);
        assert_eq!(property.get_str("default"), Dynamic::string("onCode"));
        assert_eq!(property.get_str("enum").get_str("length"), Dynamic::Number(3.0));
        assert_eq!(property.get_str("enumDescriptions").get_str("length"), Dynamic::Number(3.0));
    }
}
