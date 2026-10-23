//! Porte de `EditorAccessibilitySupport` (`editorOptions.ts`, linhas 1477 a 1513).
//!
//! A entrada é `'auto' | 'on' | 'off'` e o valor guardado é [`AccessibilitySupport`]. `auto` vira
//! `Unknown`, e `compute` troca `Unknown` pelo que o ambiente (a plataforma) informa.

use je_base::dynamic::Dynamic;

use crate::config::editor_options::{
    strings_array, AccessibilitySupport, ComputedEditorOptions, EditorOption, EditorOptionImpl, EnvironmentalOptions, Opt,
};

/// Chave tipada de `accessibilitySupport`.
pub const ACCESSIBILITY_SUPPORT: Opt<AccessibilitySupport> = Opt::new(EditorOption::AccessibilitySupport);

/// `EditorAccessibilitySupport`.
pub struct EditorAccessibilitySupport {
    schema: Dynamic,
}

impl EditorAccessibilitySupport {
    pub fn new() -> Self {
        let schema = Dynamic::object([
            ("type", Dynamic::string("string")),
            ("enum", strings_array(&["auto", "on", "off"])),
            (
                "enumDescriptions",
                strings_array(&[
                    "Use platform APIs to detect when a Screen Reader is attached.",
                    "Optimize for usage with a Screen Reader.",
                    "Assume a screen reader is not attached.",
                ]),
            ),
            ("default", Dynamic::string("auto")),
            ("tags", strings_array(&["accessibility"])),
            (
                "description",
                Dynamic::string("Controls if the UI should run in a mode where it is optimized for screen readers."),
            ),
        ]);
        Self { schema }
    }
}

impl Default for EditorAccessibilitySupport {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorAccessibilitySupport {
    type Value = AccessibilitySupport;

    fn id(&self) -> EditorOption {
        EditorOption::AccessibilitySupport
    }

    fn name(&self) -> &str {
        "accessibilitySupport"
    }

    fn default_value(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> AccessibilitySupport {
        if let Dynamic::String(text) = input {
            if *text == "auto" {
                return AccessibilitySupport::Unknown;
            }
            if *text == "off" {
                return AccessibilitySupport::Disabled;
            }
            if *text == "on" {
                return AccessibilitySupport::Enabled;
            }
        }
        self.default_value()
    }

    fn compute(&self, env: &EnvironmentalOptions, _options: &ComputedEditorOptions, value: &AccessibilitySupport) -> AccessibilitySupport {
        if *value == AccessibilitySupport::Unknown {
            // O editor lê o `accessibilitySupport` do ambiente.
            return env.accessibility_support;
        }
        *value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::option_support::testing::{environment, font_info, options_with};

    fn validate(input: Dynamic) -> AccessibilitySupport {
        EditorAccessibilitySupport::new().validate(&input)
    }

    #[test]
    fn identity_and_default() {
        let option = EditorAccessibilitySupport::new();
        assert_eq!(option.id(), EditorOption::AccessibilitySupport);
        assert_eq!(option.name(), "accessibilitySupport");
        assert_eq!(option.default_value(), AccessibilitySupport::Unknown);
        assert_eq!(ACCESSIBILITY_SUPPORT.id(), EditorOption::AccessibilitySupport);
    }

    #[test]
    fn validate_maps_the_three_strings() {
        assert_eq!(validate(Dynamic::string("auto")), AccessibilitySupport::Unknown);
        assert_eq!(validate(Dynamic::string("off")), AccessibilitySupport::Disabled);
        assert_eq!(validate(Dynamic::string("on")), AccessibilitySupport::Enabled);
    }

    #[test]
    fn validate_falls_back_to_the_default() {
        assert_eq!(validate(Dynamic::string("ON")), AccessibilitySupport::Unknown);
        assert_eq!(validate(Dynamic::string("")), AccessibilitySupport::Unknown);
        assert_eq!(validate(Dynamic::Undefined), AccessibilitySupport::Unknown);
        assert_eq!(validate(Dynamic::Null), AccessibilitySupport::Unknown);
        assert_eq!(validate(Dynamic::Bool(true)), AccessibilitySupport::Unknown);
        assert_eq!(validate(Dynamic::Number(2.0)), AccessibilitySupport::Unknown);
    }

    #[test]
    fn compute_reads_the_environment_only_when_unknown() {
        let option = EditorAccessibilitySupport::new();
        let mut env = environment(font_info(true, 14.0, "normal"));
        let options = options_with(vec![]);
        env.accessibility_support = AccessibilitySupport::Enabled;
        assert_eq!(option.compute(&env, &options, &AccessibilitySupport::Unknown), AccessibilitySupport::Enabled);
        assert_eq!(option.compute(&env, &options, &AccessibilitySupport::Disabled), AccessibilitySupport::Disabled);
        env.accessibility_support = AccessibilitySupport::Disabled;
        assert_eq!(option.compute(&env, &options, &AccessibilitySupport::Enabled), AccessibilitySupport::Enabled);
        assert_eq!(option.compute(&env, &options, &AccessibilitySupport::Unknown), AccessibilitySupport::Disabled);
    }

    #[test]
    fn schema_keeps_the_original_key_order() {
        let option = EditorAccessibilitySupport::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["type", "enum", "enumDescriptions", "default", "tags", "description"]);
        assert_eq!(schema.get_str("default"), Dynamic::string("auto"));
    }
}
