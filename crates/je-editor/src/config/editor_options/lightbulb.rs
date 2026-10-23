//! Porte de `EditorLightbulb` e `ShowLightbulbIconMode` (`editorOptions.ts`, linhas 3109 a 3169).
//!
//! `enumDescriptions` do esquema ainda não existe em [`ConfigurationPropertySchema`] (`// TODO(config)`).

use crate::config::editor_options::option_support::{object_input, string_input};
use crate::config::editor_options::{
    string_set, ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue,
};

/// Chave tipada de `lightbulb`.
pub const LIGHTBULB: Opt<EditorLightbulbOptions> = Opt::new(EditorOption::Lightbulb);

/// `ShowLightbulbIconMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowLightbulbIconMode {
    Off,
    OnCode,
    On,
}

impl ShowLightbulbIconMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            ShowLightbulbIconMode::Off => "off",
            ShowLightbulbIconMode::OnCode => "onCode",
            ShowLightbulbIconMode::On => "on",
        }
    }

    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "off" => Some(ShowLightbulbIconMode::Off),
            "onCode" => Some(ShowLightbulbIconMode::OnCode),
            "on" => Some(ShowLightbulbIconMode::On),
            _ => None,
        }
    }
}

const ALLOWED: [&str; 3] = ["off", "onCode", "on"];

/// `IEditorLightbulbOptions`. `enabled` fica em texto porque a entrada vem de `settings.json` e pode
/// trazer um valor fora do enum, que `validate` descarta.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IEditorLightbulbOptions {
    pub enabled: Option<String>,
}

/// `EditorLightbulbOptions` (`Readonly<Required<IEditorLightbulbOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorLightbulbOptions {
    pub enabled: ShowLightbulbIconMode,
}

/// `EditorLightbulb`.
pub struct EditorLightbulb {
    default_value: EditorLightbulbOptions,
    schema: OptionSchema,
}

impl EditorLightbulb {
    pub fn new() -> Self {
        let defaults = EditorLightbulbOptions { enabled: ShowLightbulbIconMode::OnCode };
        let property = ConfigurationPropertySchema {
            type_: Some("string".to_string()),
            enum_values: Some(ALLOWED.iter().map(|s| (*s).to_string()).collect()),
            default: Some(SchemaValue::String(defaults.enabled.as_str().to_string())),
            description: Some("Enables the Code Action lightbulb in the editor.".to_string()),
            ..Default::default()
        };
        Self { default_value: defaults, schema: OptionSchema::Map(vec![("editor.lightbulb.enabled".to_string(), property)]) }
    }

    /// `validate` com a interface já montada; `None` é o caso `!_input || typeof _input !== 'object'`.
    pub fn validate_options(&self, input: Option<&IEditorLightbulbOptions>) -> EditorLightbulbOptions {
        let Some(input) = input else {
            return self.default_value;
        };
        let enabled = string_set(&string_input(input.enabled.as_deref()), self.default_value.enabled.as_str(), &ALLOWED, None);
        EditorLightbulbOptions { enabled: ShowLightbulbIconMode::from_name(&enabled).unwrap_or(self.default_value.enabled) }
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

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> EditorLightbulbOptions {
        self.validate_options(object_input::<IEditorLightbulbOptions>(input).as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(enabled: &str) -> IEditorLightbulbOptions {
        IEditorLightbulbOptions { enabled: Some(enabled.to_string()) }
    }

    #[test]
    fn default_is_on_code() {
        let option = EditorLightbulb::new();
        assert_eq!(option.default_value().enabled, ShowLightbulbIconMode::OnCode);
    }

    #[test]
    fn non_objects_give_the_default() {
        let option = EditorLightbulb::new();
        assert_eq!(option.validate_options(None), option.default_value());
        for input in [OptionInput::Undefined, OptionInput::Null, OptionInput::Bool(true), OptionInput::Number(1.0), OptionInput::string("on")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
    }

    #[test]
    fn empty_object_gives_the_default() {
        let option = EditorLightbulb::new();
        assert_eq!(option.validate(&OptionInput::Other), option.default_value());
        assert_eq!(option.validate_options(Some(&IEditorLightbulbOptions::default())), option.default_value());
    }

    #[test]
    fn enabled_accepts_only_the_three_modes() {
        let option = EditorLightbulb::new();
        assert_eq!(option.validate_options(Some(&with("off"))).enabled, ShowLightbulbIconMode::Off);
        assert_eq!(option.validate_options(Some(&with("on"))).enabled, ShowLightbulbIconMode::On);
        assert_eq!(option.validate_options(Some(&with("onCode"))).enabled, ShowLightbulbIconMode::OnCode);
        assert_eq!(option.validate_options(Some(&with("oncode"))).enabled, ShowLightbulbIconMode::OnCode);
        assert_eq!(option.validate_options(Some(&with("true"))).enabled, ShowLightbulbIconMode::OnCode);
    }

    #[test]
    fn schema_has_the_enabled_key() {
        let option = EditorLightbulb::new();
        let Some(OptionSchema::Map(entries)) = option.schema() else { panic!("sem esquema") };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "editor.lightbulb.enabled");
        assert_eq!(entries[0].1.default, Some(SchemaValue::String("onCode".to_string())));
        assert_eq!(entries[0].1.enum_values.as_ref().map(Vec::len), Some(3));
    }
}
