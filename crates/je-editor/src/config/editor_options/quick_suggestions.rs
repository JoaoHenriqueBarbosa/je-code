//! Porte de `EditorQuickSuggestions` e `QuickSuggestionsValue` (`editorOptions.ts`, linhas 3792 a 3914).
//!
//! O esquema do original é um `anyOf` (booleano, enum de texto e objeto), que
//! [`ConfigurationPropertySchema`] ainda não representa: `schema()` devolve `None`
//! (`// TODO(config)`: escrever o `anyOf` quando o `IConfigurationPropertySchema` de `je-base` existir).

use crate::config::editor_options::option_support::{input_or_undefined, object_input};
use crate::config::editor_options::{string_set, EditorOption, EditorOptionImpl, Opt, OptionInput};

/// Chave tipada de `quickSuggestions`.
pub const QUICK_SUGGESTIONS: Opt<InternalQuickSuggestionsOptions> = Opt::new(EditorOption::QuickSuggestions);

/// `QuickSuggestionsValue`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickSuggestionsValue {
    On,
    Inline,
    Off,
    OffWhenInlineCompletions,
}

impl QuickSuggestionsValue {
    pub const fn as_str(self) -> &'static str {
        match self {
            QuickSuggestionsValue::On => "on",
            QuickSuggestionsValue::Inline => "inline",
            QuickSuggestionsValue::Off => "off",
            QuickSuggestionsValue::OffWhenInlineCompletions => "offWhenInlineCompletions",
        }
    }

    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "on" => Some(QuickSuggestionsValue::On),
            "inline" => Some(QuickSuggestionsValue::Inline),
            "off" => Some(QuickSuggestionsValue::Off),
            "offWhenInlineCompletions" => Some(QuickSuggestionsValue::OffWhenInlineCompletions),
            _ => None,
        }
    }

    /// Booleano vira `'on'` ou `'off'`.
    pub const fn from_bool(value: bool) -> Self {
        if value { QuickSuggestionsValue::On } else { QuickSuggestionsValue::Off }
    }
}

const ALLOWED: [&str; 4] = ["on", "inline", "off", "offWhenInlineCompletions"];

/// `IQuickSuggestionsOptions`. Cada campo é `boolean | QuickSuggestionsValue`, e por isso fica como
/// [`OptionInput`] (`Bool` ou `String`; qualquer outro tipo cai no padrão do campo).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IQuickSuggestionsOptions {
    pub other: Option<OptionInput>,
    pub comments: Option<OptionInput>,
    pub strings: Option<OptionInput>,
}

/// `boolean | QuickSuggestionsValue | IQuickSuggestionsOptions`.
#[derive(Clone, Debug, PartialEq)]
pub enum QuickSuggestionsInput {
    Bool(bool),
    /// Atalho em texto: o mesmo valor para os três tipos de token (texto fora do enum dá o padrão de `other`).
    Value(String),
    Options(IQuickSuggestionsOptions),
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
}

impl EditorQuickSuggestions {
    pub fn new() -> Self {
        Self {
            default_value: InternalQuickSuggestionsOptions {
                other: QuickSuggestionsValue::OffWhenInlineCompletions,
                comments: QuickSuggestionsValue::Off,
                strings: QuickSuggestionsValue::Off,
            },
        }
    }

    fn validate_text(input: &OptionInput, default_value: QuickSuggestionsValue) -> QuickSuggestionsValue {
        let validated = string_set(input, default_value.as_str(), &ALLOWED, None);
        QuickSuggestionsValue::from_name(&validated).unwrap_or(default_value)
    }

    fn validate_field(field: Option<&OptionInput>, default_value: QuickSuggestionsValue) -> QuickSuggestionsValue {
        match field {
            Some(OptionInput::Bool(value)) => QuickSuggestionsValue::from_bool(*value),
            other => Self::validate_text(&input_or_undefined(other), default_value),
        }
    }

    /// `validate` com a entrada tipada; `None` é o caso `!input || typeof input !== 'object'`.
    pub fn validate_input(&self, input: Option<&QuickSuggestionsInput>) -> InternalQuickSuggestionsOptions {
        match input {
            // booleano: tudo ligado ou tudo desligado
            Some(QuickSuggestionsInput::Bool(value)) => InternalQuickSuggestionsOptions::all(QuickSuggestionsValue::from_bool(*value)),
            // atalho em texto: o mesmo valor para todos os tipos de token
            Some(QuickSuggestionsInput::Value(text)) => {
                InternalQuickSuggestionsOptions::all(Self::validate_text(&OptionInput::string(text), self.default_value.other))
            }
            Some(QuickSuggestionsInput::Options(options)) => InternalQuickSuggestionsOptions {
                other: Self::validate_field(options.other.as_ref(), self.default_value.other),
                comments: Self::validate_field(options.comments.as_ref(), self.default_value.comments),
                strings: Self::validate_field(options.strings.as_ref(), self.default_value.strings),
            },
            None => self.default_value,
        }
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

    fn validate(&self, input: &OptionInput) -> InternalQuickSuggestionsOptions {
        let typed = match input {
            OptionInput::Bool(value) => Some(QuickSuggestionsInput::Bool(*value)),
            OptionInput::String(text) => Some(QuickSuggestionsInput::Value(text.clone())),
            other => object_input::<IQuickSuggestionsOptions>(other).map(QuickSuggestionsInput::Options),
        };
        self.validate_input(typed.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::QuickSuggestionsValue::{Inline, Off, OffWhenInlineCompletions, On};
    use super::*;

    fn all(value: QuickSuggestionsValue) -> InternalQuickSuggestionsOptions {
        InternalQuickSuggestionsOptions { other: value, comments: value, strings: value }
    }

    #[test]
    fn default_value() {
        let value = EditorQuickSuggestions::new().default_value();
        assert_eq!((value.other, value.comments, value.strings), (OffWhenInlineCompletions, Off, Off));
    }

    #[test]
    fn booleans_set_all_three() {
        let option = EditorQuickSuggestions::new();
        assert_eq!(option.validate(&OptionInput::Bool(true)), all(On));
        assert_eq!(option.validate(&OptionInput::Bool(false)), all(Off));
    }

    #[test]
    fn strings_set_all_three_or_fall_back_to_the_default_of_other() {
        let option = EditorQuickSuggestions::new();
        assert_eq!(option.validate(&OptionInput::string("inline")), all(Inline));
        assert_eq!(option.validate(&OptionInput::string("on")), all(On));
        assert_eq!(option.validate(&OptionInput::string("offWhenInlineCompletions")), all(OffWhenInlineCompletions));
        // Fora do enum: o padrão de `other`, aplicado aos três.
        assert_eq!(option.validate(&OptionInput::string("bogus")), all(OffWhenInlineCompletions));
        assert_eq!(option.validate(&OptionInput::string("")), all(OffWhenInlineCompletions));
    }

    #[test]
    fn invalid_inputs_give_the_default() {
        let option = EditorQuickSuggestions::new();
        for input in [OptionInput::Undefined, OptionInput::Null, OptionInput::Number(1.0)] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate_input(None), option.default_value());
    }

    #[test]
    fn empty_object_gives_the_per_field_defaults() {
        let option = EditorQuickSuggestions::new();
        assert_eq!(option.validate(&OptionInput::Other), option.default_value());
        assert_eq!(option.validate_input(Some(&QuickSuggestionsInput::Options(IQuickSuggestionsOptions::default()))), option.default_value());
    }

    #[test]
    fn object_fields_accept_booleans_strings_and_fall_back_individually() {
        let option = EditorQuickSuggestions::new();
        let input = QuickSuggestionsInput::Options(IQuickSuggestionsOptions {
            other: Some(OptionInput::Bool(false)),
            comments: Some(OptionInput::string("inline")),
            strings: Some(OptionInput::Number(5.0)),
        });
        let value = option.validate_input(Some(&input));
        assert_eq!((value.other, value.comments, value.strings), (Off, Inline, Off));

        let input = QuickSuggestionsInput::Options(IQuickSuggestionsOptions {
            other: Some(OptionInput::string("bogus")),
            comments: Some(OptionInput::Bool(true)),
            strings: Some(OptionInput::string("on")),
        });
        let value = option.validate_input(Some(&input));
        assert_eq!((value.other, value.comments, value.strings), (OffWhenInlineCompletions, On, On));
    }

    #[test]
    fn object_fields_use_their_own_defaults() {
        let option = EditorQuickSuggestions::new();
        let input = QuickSuggestionsInput::Options(IQuickSuggestionsOptions {
            other: Some(OptionInput::string("zzz")),
            comments: Some(OptionInput::string("zzz")),
            strings: Some(OptionInput::string("zzz")),
        });
        assert_eq!(option.validate_input(Some(&input)), option.default_value());
    }

    #[test]
    fn identity_and_schema() {
        let option = EditorQuickSuggestions::new();
        assert_eq!(option.id(), EditorOption::QuickSuggestions);
        assert_eq!(option.name(), "quickSuggestions");
        assert!(option.schema().is_none());
    }
}
