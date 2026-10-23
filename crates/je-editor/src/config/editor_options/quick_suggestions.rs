//! Porte de `EditorQuickSuggestions` e `QuickSuggestionsValue` (`editorOptions.ts`, linhas 3792 a 3914).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{strings_array, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `quickSuggestions`.
pub const QUICK_SUGGESTIONS: Opt<InternalQuickSuggestionsOptions> = Opt::new(EditorOption::QuickSuggestions);

string_enum! {
    /// `QuickSuggestionsValue`.
    pub enum QuickSuggestionsValue {
        On => "on",
        Inline => "inline",
        Off => "off",
        OffWhenInlineCompletions => "offWhenInlineCompletions",
    }
}

impl QuickSuggestionsValue {
    /// Booleano vira `'on'` ou `'off'`.
    pub const fn from_bool(value: bool) -> Self {
        if value { QuickSuggestionsValue::On } else { QuickSuggestionsValue::Off }
    }

    /// Campo de `IQuickSuggestionsOptions`: booleano vira `'on'`/`'off'`, o resto passa por `stringSet`.
    fn validate_field(field: &Dynamic, default_value: Self) -> Self {
        match field {
            Dynamic::Bool(value) => Self::from_bool(*value),
            other => Self::validate(other, default_value),
        }
    }
}

/// `InternalQuickSuggestionsOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalQuickSuggestionsOptions {
    pub other: QuickSuggestionsValue,
    pub comments: QuickSuggestionsValue,
    pub strings: QuickSuggestionsValue,
}

impl InternalQuickSuggestionsOptions {
    /// O mesmo valor para os três tipos de token.
    const fn all(value: QuickSuggestionsValue) -> Self {
        Self { other: value, comments: value, strings: value }
    }
}

/// `EditorQuickSuggestions`.
pub struct EditorQuickSuggestions {
    default_value: InternalQuickSuggestionsOptions,
    schema: Dynamic,
}

impl EditorQuickSuggestions {
    pub fn new() -> Self {
        let defaults = InternalQuickSuggestionsOptions {
            other: QuickSuggestionsValue::OffWhenInlineCompletions,
            comments: QuickSuggestionsValue::Off,
            strings: QuickSuggestionsValue::Off,
        };
        // `types` é compartilhado pelos três campos do objeto, como no original.
        let types = Dynamic::array([
            Dynamic::object([("type", Dynamic::string("boolean"))]),
            Dynamic::object([
                ("type", Dynamic::string("string")),
                ("enum", strings_array(QuickSuggestionsValue::ALLOWED)),
                (
                    "enumDescriptions",
                    Dynamic::array([
                        Dynamic::string("Quick suggestions show inside the suggest widget"),
                        Dynamic::string("Quick suggestions show as ghost text"),
                        Dynamic::string("Quick suggestions are disabled"),
                        Dynamic::string("Quick suggestions are disabled when inline completions are showing"),
                    ]),
                ),
            ]),
        ]);
        let field = |default_value: QuickSuggestionsValue, description: &str| {
            Dynamic::object([
                ("anyOf", types.clone()),
                ("default", Dynamic::string(default_value.as_str())),
                ("description", Dynamic::string(description)),
            ])
        };
        let schema = Dynamic::object([
            (
                "anyOf",
                Dynamic::array([
                    Dynamic::object([("type", Dynamic::string("boolean"))]),
                    Dynamic::object([
                        ("type", Dynamic::string("string")),
                        ("enum", strings_array(QuickSuggestionsValue::ALLOWED)),
                        (
                            "enumDescriptions",
                            Dynamic::array([
                                Dynamic::string("Quick suggestions are enabled for all token types"),
                                Dynamic::string("Quick suggestions show as ghost text for all token types"),
                                Dynamic::string("Quick suggestions are disabled for all token types"),
                                Dynamic::string("Quick suggestions are disabled for all token types when inline completions are showing"),
                            ]),
                        ),
                    ]),
                    Dynamic::object([
                        ("type", Dynamic::string("object")),
                        ("additionalProperties", Dynamic::Bool(false)),
                        (
                            "properties",
                            Dynamic::object([
                                ("strings", field(defaults.strings, "Enable quick suggestions inside strings.")),
                                ("comments", field(defaults.comments, "Enable quick suggestions inside comments.")),
                                ("other", field(defaults.other, "Enable quick suggestions outside of strings and comments.")),
                            ]),
                        ),
                    ]),
                ]),
            ),
            (
                "default",
                Dynamic::object([
                    ("other", Dynamic::string(defaults.other.as_str())),
                    ("comments", Dynamic::string(defaults.comments.as_str())),
                    ("strings", Dynamic::string(defaults.strings.as_str())),
                ]),
            ),
            (
                "markdownDescription",
                Dynamic::string("Controls whether suggestions should automatically show up while typing. This can be controlled for typing in comments, strings, and other code. Quick suggestion can be configured to show as ghost text or with the suggest widget. Also be aware of the `#editor.suggestOnTriggerCharacters#`-setting which controls if suggestions are triggered by special characters."),
            ),
            ("experiment", Dynamic::object([("mode", Dynamic::string("auto"))])),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorQuickSuggestions {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorQuickSuggestions {
    type Value = InternalQuickSuggestionsOptions;

    fn id(&self) -> EditorOption {
        EditorOption::QuickSuggestions
    }

    fn name(&self) -> &str {
        "quickSuggestions"
    }

    fn default_value(&self) -> InternalQuickSuggestionsOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> InternalQuickSuggestionsOptions {
        match input {
            // booleano: tudo ligado ou tudo desligado
            Dynamic::Bool(value) => return InternalQuickSuggestionsOptions::all(QuickSuggestionsValue::from_bool(*value)),
            // atalho em texto: o mesmo valor para todos os tipos de token
            Dynamic::String(_) => return InternalQuickSuggestionsOptions::all(QuickSuggestionsValue::validate(input, self.default_value.other)),
            _ => {}
        }
        if !input.is_truthy() || input.type_of() != "object" {
            // entrada inválida
            return self.default_value;
        }
        InternalQuickSuggestionsOptions {
            other: QuickSuggestionsValue::validate_field(&input.get_str("other"), self.default_value.other),
            comments: QuickSuggestionsValue::validate_field(&input.get_str("comments"), self.default_value.comments),
            strings: QuickSuggestionsValue::validate_field(&input.get_str("strings"), self.default_value.strings),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QuickSuggestionsValue::{Inline, Off, OffWhenInlineCompletions, On};
    use super::*;

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn all(value: QuickSuggestionsValue) -> InternalQuickSuggestionsOptions {
        InternalQuickSuggestionsOptions::all(value)
    }

    fn fields(other: Dynamic, comments: Dynamic, strings: Dynamic) -> Dynamic {
        Dynamic::object([("other", other), ("comments", comments), ("strings", strings)])
    }

    #[test]
    fn default_value() {
        let value = EditorQuickSuggestions::new().default_value();
        assert_eq!((value.other, value.comments, value.strings), (OffWhenInlineCompletions, Off, Off));
    }

    #[test]
    fn booleans_set_all_three() {
        let option = EditorQuickSuggestions::new();
        assert_eq!(option.validate(&Dynamic::Bool(true)), all(On));
        assert_eq!(option.validate(&Dynamic::Bool(false)), all(Off));
    }

    #[test]
    fn strings_set_all_three_or_fall_back_to_the_default_of_other() {
        let option = EditorQuickSuggestions::new();
        assert_eq!(option.validate(&text("inline")), all(Inline));
        assert_eq!(option.validate(&text("on")), all(On));
        assert_eq!(option.validate(&text("offWhenInlineCompletions")), all(OffWhenInlineCompletions));
        // Fora do enum: o padrão de `other`, aplicado aos três.
        assert_eq!(option.validate(&text("bogus")), all(OffWhenInlineCompletions));
        assert_eq!(option.validate(&text("")), all(OffWhenInlineCompletions));
    }

    #[test]
    fn invalid_inputs_give_the_default() {
        let option = EditorQuickSuggestions::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Number(1.0), Dynamic::Number(0.0)] {
            assert_eq!(option.validate(&input), option.default_value());
        }
    }

    #[test]
    fn empty_object_gives_the_per_field_defaults() {
        let option = EditorQuickSuggestions::new();
        assert_eq!(option.validate(&Dynamic::object::<&str>([])), option.default_value());
        // Um array também é `typeof 'object'`.
        assert_eq!(option.validate(&Dynamic::array([])), option.default_value());
    }

    #[test]
    fn object_fields_accept_booleans_strings_and_fall_back_individually() {
        let option = EditorQuickSuggestions::new();
        let value = option.validate(&fields(Dynamic::Bool(false), text("inline"), Dynamic::Number(5.0)));
        assert_eq!((value.other, value.comments, value.strings), (Off, Inline, Off));

        let value = option.validate(&fields(text("bogus"), Dynamic::Bool(true), text("on")));
        assert_eq!((value.other, value.comments, value.strings), (OffWhenInlineCompletions, On, On));
    }

    #[test]
    fn object_fields_use_their_own_defaults() {
        let option = EditorQuickSuggestions::new();
        assert_eq!(option.validate(&fields(text("zzz"), text("zzz"), text("zzz"))), option.default_value());
        let value = option.validate(&Dynamic::object([("comments", text("on"))]));
        assert_eq!((value.other, value.comments, value.strings), (OffWhenInlineCompletions, On, Off));
    }

    #[test]
    fn schema_keeps_the_original_shape_and_order() {
        let option = EditorQuickSuggestions::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["anyOf", "default", "markdownDescription", "experiment"]);
        let any_of = schema.get_str("anyOf");
        assert_eq!(any_of.get_str("length"), Dynamic::Number(3.0));
        let object = any_of.get_str("2");
        assert_eq!(object.own_keys(), vec!["type", "additionalProperties", "properties"]);
        let properties = object.get_str("properties");
        assert_eq!(properties.own_keys(), vec!["strings", "comments", "other"]);
        assert_eq!(properties.get_str("other").own_keys(), vec!["anyOf", "default", "description"]);
        assert_eq!(properties.get_str("other").get_str("default"), text("offWhenInlineCompletions"));
        // `types` é a mesma referência nos três campos.
        assert!(properties.get_str("strings").get_str("anyOf").strict_equals(&properties.get_str("other").get_str("anyOf")));
        assert_eq!(schema.get_str("default").own_keys(), vec!["other", "comments", "strings"]);
        assert_eq!(schema.get_str("experiment").get_str("mode"), text("auto"));
    }
}
