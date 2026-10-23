//! Porte de `EditorInlayHints` (`editorOptions.ts`, linhas 3247 a 3350).

use je_base::dynamic::{Dynamic, Truthy};
use je_base::platform::Platform;

use crate::config::editor_options::{boolean, clamped_int, string, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `inlayHints`.
pub const INLAY_HINTS: Opt<EditorInlayHintsOptions> = Opt::new(EditorOption::InlayHints);

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

string_enum! {
    /// `'on' | 'off' | 'offUnlessPressed' | 'onUnlessPressed'`.
    pub enum InlayHintsEnabled {
        On => "on",
        Off => "off",
        OffUnlessPressed => "offUnlessPressed",
        OnUnlessPressed => "onUnlessPressed",
    }
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
    schema: Dynamic,
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
        let modifier = if Platform::detect().is_macintosh { "Ctrl+Option" } else { "Ctrl+Alt" };
        let schema = Dynamic::object([
            (
                "editor.inlayHints.enabled",
                Dynamic::object([
                    ("type", Dynamic::string("string")),
                    ("default", Dynamic::string(defaults.enabled.as_str())),
                    ("description", Dynamic::string("Enables the inlay hints in the editor.")),
                    (
                        "enum",
                        Dynamic::array(["on", "onUnlessPressed", "offUnlessPressed", "off"].map(Dynamic::string)),
                    ),
                    (
                        "markdownEnumDescriptions",
                        Dynamic::array([
                            Dynamic::string("Inlay hints are enabled"),
                            Dynamic::string(&format!("Inlay hints are showing by default and hide when holding {modifier}")),
                            Dynamic::string(&format!("Inlay hints are hidden by default and show when holding {modifier}")),
                            Dynamic::string("Inlay hints are disabled"),
                        ]),
                    ),
                ]),
            ),
            (
                "editor.inlayHints.fontSize",
                Dynamic::object([
                    ("type", Dynamic::string("number")),
                    ("default", defaults.font_size.into()),
                    (
                        "markdownDescription",
                        Dynamic::string("Controls font size of inlay hints in the editor. As default the `#editor.fontSize#` is used when the configured value is less than `5` or greater than the editor font size."),
                    ),
                ]),
            ),
            (
                "editor.inlayHints.fontFamily",
                Dynamic::object([
                    ("type", Dynamic::string("string")),
                    ("default", Dynamic::string(&defaults.font_family)),
                    (
                        "markdownDescription",
                        Dynamic::string("Controls font family of inlay hints in the editor. When set to empty, the `#editor.fontFamily#` is used."),
                    ),
                ]),
            ),
            (
                "editor.inlayHints.padding",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", defaults.padding.into()),
                    ("description", Dynamic::string("Enables the padding around the inlay hints in the editor.")),
                ]),
            ),
            (
                "editor.inlayHints.maximumLength",
                Dynamic::object([
                    ("type", Dynamic::string("number")),
                    ("default", defaults.maximum_length.into()),
                    (
                        "markdownDescription",
                        Dynamic::string("Maximum overall length of inlay hints, for a single line, before they get truncated by the editor. Set to `0` to never truncate"),
                    ),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
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

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorInlayHintsOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value.clone();
        }
        let defaults = &self.default_value;
        let enabled = match input.get_str("enabled") {
            // `typeof input.enabled === 'boolean'`
            Dynamic::Bool(enabled) => Dynamic::string(if enabled { "on" } else { "off" }),
            other => other,
        };
        EditorInlayHintsOptions {
            enabled: InlayHintsEnabled::validate(&enabled, defaults.enabled),
            font_size: clamped_int(&input.get_str("fontSize"), defaults.font_size, 0.0, 100.0),
            font_family: string(&input.get_str("fontFamily"), &defaults.font_family),
            padding: boolean(&input.get_str("padding"), defaults.padding),
            maximum_length: clamped_int(&input.get_str("maximumLength"), defaults.maximum_length, 0.0, MAX_SAFE_INTEGER),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn num(n: f64) -> Dynamic {
        Dynamic::Number(n)
    }

    fn validate_field(key: &str, value: Dynamic) -> EditorInlayHintsOptions {
        EditorInlayHints::new().validate(&Dynamic::object([(key, value)]))
    }

    fn enabled(input: Dynamic) -> InlayHintsEnabled {
        validate_field("enabled", input).enabled
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
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), num(1.0), text("on")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate(&Dynamic::object::<&str>([])), option.default_value());
    }

    #[test]
    fn enabled_accepts_booleans_and_the_four_strings() {
        assert_eq!(enabled(Dynamic::Bool(true)), InlayHintsEnabled::On);
        assert_eq!(enabled(Dynamic::Bool(false)), InlayHintsEnabled::Off);
        assert_eq!(enabled(text("offUnlessPressed")), InlayHintsEnabled::OffUnlessPressed);
        assert_eq!(enabled(text("onUnlessPressed")), InlayHintsEnabled::OnUnlessPressed);
        assert_eq!(enabled(text("off")), InlayHintsEnabled::Off);
        assert_eq!(enabled(text("nope")), InlayHintsEnabled::On);
        assert_eq!(enabled(num(1.0)), InlayHintsEnabled::On);
        assert_eq!(enabled(Dynamic::Null), InlayHintsEnabled::On);
    }

    #[test]
    fn numbers_are_clamped_integers() {
        assert_eq!(validate_field("fontSize", num(250.0)).font_size, 100.0);
        assert_eq!(validate_field("fontSize", num(12.8)).font_size, 12.0);
        assert_eq!(validate_field("fontSize", num(f64::NAN)).font_size, 0.0);
        assert_eq!(validate_field("fontSize", text("14px")).font_size, 14.0);
        assert_eq!(validate_field("maximumLength", num(-3.0)).maximum_length, 0.0);
        assert_eq!(validate_field("maximumLength", num(80.0)).maximum_length, 80.0);
    }

    #[test]
    fn huge_maximum_length_wraps_like_the_int32_cast() {
        // O clamp deixa passar 1e12 e `r | 0` dá a volta em 32 bits, como no JavaScript.
        assert_eq!(validate_field("maximumLength", num(1e12)).maximum_length, -727_379_968.0);
    }

    #[test]
    fn font_family_and_padding() {
        let option = EditorInlayHints::new();
        let value = option.validate(&Dynamic::object([("fontFamily", text("Fira Code")), ("padding", Dynamic::Bool(true))]));
        assert_eq!(value.font_family, "Fira Code");
        assert!(value.padding);
        assert_eq!(validate_field("fontFamily", num(1.0)).font_family, "");
        assert!(!validate_field("padding", text("false")).padding);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = EditorInlayHints::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec![
                "editor.inlayHints.enabled",
                "editor.inlayHints.fontSize",
                "editor.inlayHints.fontFamily",
                "editor.inlayHints.padding",
                "editor.inlayHints.maximumLength"
            ]
        );
        let enabled = schema.get_str("editor.inlayHints.enabled");
        assert_eq!(enabled.own_keys(), vec!["type", "default", "description", "enum", "markdownEnumDescriptions"]);
        assert_eq!(enabled.get_str("enum").get_str("1"), text("onUnlessPressed"));
        assert_eq!(enabled.get_str("markdownEnumDescriptions").get_str("length"), num(4.0));
        assert_eq!(schema.get_str("editor.inlayHints.maximumLength").get_str("default"), num(43.0));
    }
}
