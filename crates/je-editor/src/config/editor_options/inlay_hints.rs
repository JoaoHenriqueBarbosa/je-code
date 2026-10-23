//! Porte de `EditorInlayHints` (`editorOptions.ts`, linhas 3247 a 3350).
//!
//! As descrições `markdownEnumDescriptions` do esquema ainda não existem em
//! [`ConfigurationPropertySchema`] (`// TODO(config)`).

use crate::config::editor_options::option_support::{bool_input, input_or_undefined, number_input, object_input, string_input};
use crate::config::editor_options::{
    boolean, clamped_int, string, string_set, ConfigurationPropertySchema, EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema, SchemaValue,
};

/// Chave tipada de `inlayHints`.
pub const INLAY_HINTS: Opt<EditorInlayHintsOptions> = Opt::new(EditorOption::InlayHints);

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `'on' | 'off' | 'offUnlessPressed' | 'onUnlessPressed'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlayHintsEnabled {
    On,
    Off,
    OffUnlessPressed,
    OnUnlessPressed,
}

impl InlayHintsEnabled {
    pub const fn as_str(self) -> &'static str {
        match self {
            InlayHintsEnabled::On => "on",
            InlayHintsEnabled::Off => "off",
            InlayHintsEnabled::OffUnlessPressed => "offUnlessPressed",
            InlayHintsEnabled::OnUnlessPressed => "onUnlessPressed",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "on" => Some(InlayHintsEnabled::On),
            "off" => Some(InlayHintsEnabled::Off),
            "offUnlessPressed" => Some(InlayHintsEnabled::OffUnlessPressed),
            "onUnlessPressed" => Some(InlayHintsEnabled::OnUnlessPressed),
            _ => None,
        }
    }
}

/// Valores aceitos por `validate`, na ordem do original.
const ALLOWED: [&str; 4] = ["on", "off", "offUnlessPressed", "onUnlessPressed"];

/// `IEditorInlayHintsOptions`. `enabled` é [`OptionInput`] porque o `validate` do original também
/// aceita um booleano (`true` vira `'on'`, `false` vira `'off'`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IEditorInlayHintsOptions {
    pub enabled: Option<OptionInput>,
    pub font_size: Option<f64>,
    pub font_family: Option<String>,
    pub padding: Option<bool>,
    pub maximum_length: Option<f64>,
}

/// `EditorInlayHintsOptions` (`Readonly<Required<IEditorInlayHintsOptions>>`).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorInlayHintsOptions {
    pub enabled: InlayHintsEnabled,
    pub font_size: f64,
    pub font_family: String,
    pub padding: bool,
    pub maximum_length: f64,
}

/// `EditorInlayHints`.
pub struct EditorInlayHints {
    default_value: EditorInlayHintsOptions,
    schema: OptionSchema,
}

impl EditorInlayHints {
    pub fn new() -> Self {
        let defaults = EditorInlayHintsOptions {
            enabled: InlayHintsEnabled::On,
            font_size: 0.0,
            font_family: String::new(),
            padding: false,
            maximum_length: 43.0,
        };
        let schema = OptionSchema::Map(vec![
            (
                "editor.inlayHints.enabled".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("string".to_string()),
                    default: Some(SchemaValue::String(defaults.enabled.as_str().to_string())),
                    description: Some("Enables the inlay hints in the editor.".to_string()),
                    enum_values: Some(["on", "onUnlessPressed", "offUnlessPressed", "off"].iter().map(|s| (*s).to_string()).collect()),
                    ..Default::default()
                },
            ),
            (
                "editor.inlayHints.fontSize".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("number".to_string()),
                    default: Some(SchemaValue::Number(defaults.font_size)),
                    markdown_description: Some(
                        "Controls font size of inlay hints in the editor. As default the `#editor.fontSize#` is used when the configured value is less than `5` or greater than the editor font size."
                            .to_string(),
                    ),
                    ..Default::default()
                },
            ),
            (
                "editor.inlayHints.fontFamily".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("string".to_string()),
                    default: Some(SchemaValue::String(defaults.font_family.clone())),
                    markdown_description: Some(
                        "Controls font family of inlay hints in the editor. When set to empty, the `#editor.fontFamily#` is used.".to_string(),
                    ),
                    ..Default::default()
                },
            ),
            (
                "editor.inlayHints.padding".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("boolean".to_string()),
                    default: Some(SchemaValue::Bool(defaults.padding)),
                    description: Some("Enables the padding around the inlay hints in the editor.".to_string()),
                    ..Default::default()
                },
            ),
            (
                "editor.inlayHints.maximumLength".to_string(),
                ConfigurationPropertySchema {
                    type_: Some("number".to_string()),
                    default: Some(SchemaValue::Number(defaults.maximum_length)),
                    markdown_description: Some(
                        "Maximum overall length of inlay hints, for a single line, before they get truncated by the editor. Set to `0` to never truncate"
                            .to_string(),
                    ),
                    ..Default::default()
                },
            ),
        ]);
        Self { default_value: defaults, schema }
    }

    /// `validate` com a interface já montada; `None` é o caso `!_input || typeof _input !== 'object'`.
    pub fn validate_options(&self, input: Option<&IEditorInlayHintsOptions>) -> EditorInlayHintsOptions {
        let Some(input) = input else {
            return self.default_value.clone();
        };
        let defaults = &self.default_value;
        let enabled_input = match &input.enabled {
            // `typeof input.enabled === 'boolean'`
            Some(OptionInput::Bool(enabled)) => OptionInput::string(if *enabled { "on" } else { "off" }),
            other => input_or_undefined(other.as_ref()),
        };
        let enabled = string_set(&enabled_input, defaults.enabled.as_str(), &ALLOWED, None);
        EditorInlayHintsOptions {
            enabled: InlayHintsEnabled::from_str(&enabled).unwrap_or(defaults.enabled),
            font_size: clamped_int(&number_input(input.font_size), defaults.font_size, 0.0, 100.0),
            font_family: string(&string_input(input.font_family.as_deref()), &defaults.font_family),
            padding: boolean(&bool_input(input.padding), defaults.padding),
            maximum_length: clamped_int(&number_input(input.maximum_length), defaults.maximum_length, 0.0, MAX_SAFE_INTEGER),
        }
    }
}

impl Default for EditorInlayHints {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorInlayHints {
    type Value = EditorInlayHintsOptions;

    fn id(&self) -> EditorOption {
        EditorOption::InlayHints
    }

    fn name(&self) -> &str {
        "inlayHints"
    }

    fn default_value(&self) -> EditorInlayHintsOptions {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, input: &OptionInput) -> EditorInlayHintsOptions {
        self.validate_options(object_input::<IEditorInlayHintsOptions>(input).as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(input: OptionInput) -> InlayHintsEnabled {
        let option = EditorInlayHints::new();
        option.validate_options(Some(&IEditorInlayHintsOptions { enabled: Some(input), ..Default::default() })).enabled
    }

    #[test]
    fn defaults() {
        let value = EditorInlayHints::new().default_value();
        assert_eq!(value.enabled, InlayHintsEnabled::On);
        assert_eq!((value.font_size, value.maximum_length), (0.0, 43.0));
        assert!(value.font_family.is_empty() && !value.padding);
    }

    #[test]
    fn non_objects_and_empty_objects_give_the_default() {
        let option = EditorInlayHints::new();
        assert_eq!(option.validate_options(None), option.default_value());
        for input in [OptionInput::Undefined, OptionInput::Null, OptionInput::Bool(true), OptionInput::Number(1.0), OptionInput::string("on")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate(&OptionInput::Other), option.default_value());
    }

    #[test]
    fn enabled_accepts_booleans_and_the_four_strings() {
        assert_eq!(enabled(OptionInput::Bool(true)), InlayHintsEnabled::On);
        assert_eq!(enabled(OptionInput::Bool(false)), InlayHintsEnabled::Off);
        assert_eq!(enabled(OptionInput::string("offUnlessPressed")), InlayHintsEnabled::OffUnlessPressed);
        assert_eq!(enabled(OptionInput::string("onUnlessPressed")), InlayHintsEnabled::OnUnlessPressed);
        assert_eq!(enabled(OptionInput::string("off")), InlayHintsEnabled::Off);
        assert_eq!(enabled(OptionInput::string("nope")), InlayHintsEnabled::On);
        assert_eq!(enabled(OptionInput::Number(1.0)), InlayHintsEnabled::On);
        assert_eq!(enabled(OptionInput::Null), InlayHintsEnabled::On);
    }

    #[test]
    fn numbers_are_clamped_integers() {
        let option = EditorInlayHints::new();
        let value = option.validate_options(Some(&IEditorInlayHintsOptions {
            font_size: Some(250.0),
            maximum_length: Some(-3.0),
            ..Default::default()
        }));
        assert_eq!((value.font_size, value.maximum_length), (100.0, 0.0));
        let value = option.validate_options(Some(&IEditorInlayHintsOptions { font_size: Some(12.8), maximum_length: Some(80.0), ..Default::default() }));
        assert_eq!((value.font_size, value.maximum_length), (12.0, 80.0));
        let value = option.validate_options(Some(&IEditorInlayHintsOptions { font_size: Some(f64::NAN), ..Default::default() }));
        assert_eq!(value.font_size, 0.0);
    }

    #[test]
    fn huge_maximum_length_wraps_like_the_int32_cast() {
        // `Number.MAX_SAFE_INTEGER | 0` é -1 no JavaScript.
        let option = EditorInlayHints::new();
        let value = option.validate_options(Some(&IEditorInlayHintsOptions { maximum_length: Some(1e15), ..Default::default() }));
        assert_eq!(value.maximum_length, f64::from((1e15_f64 as i64 & 0xFFFF_FFFF) as u32 as i32));
    }

    #[test]
    fn font_family_and_padding() {
        let option = EditorInlayHints::new();
        let value = option.validate_options(Some(&IEditorInlayHintsOptions {
            font_family: Some("Fira Code".to_string()),
            padding: Some(true),
            ..Default::default()
        }));
        assert_eq!(value.font_family, "Fira Code");
        assert!(value.padding);
    }

    #[test]
    fn schema_has_five_keys_with_the_original_enum_order() {
        let option = EditorInlayHints::new();
        let Some(OptionSchema::Map(entries)) = option.schema() else { panic!("sem esquema") };
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].0, "editor.inlayHints.enabled");
        assert_eq!(
            entries[0].1.enum_values.as_deref(),
            Some(&["on".to_string(), "onUnlessPressed".to_string(), "offUnlessPressed".to_string(), "off".to_string()][..])
        );
        assert_eq!(entries[4].1.default, Some(SchemaValue::Number(43.0)));
    }
}
