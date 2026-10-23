//! Porte de `EditorParameterHints` (`editorOptions.ts`, linhas 3697 a 3756).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `parameterHints`.
pub const PARAMETER_HINTS: Opt<InternalParameterHintOptions> = Opt::new(EditorOption::ParameterHints);

/// `InternalParameterHintOptions` (`Readonly<Required<IEditorParameterHintOptions>>`): `enabled` liga as
/// dicas de parâmetro e `cycle` liga a navegação cíclica entre elas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalParameterHintOptions {
    pub enabled: bool,
    pub cycle: bool,
}

/// `EditorParameterHints`.
pub struct EditorParameterHints {
    default_value: InternalParameterHintOptions,
    schema: Dynamic,
}

impl EditorParameterHints {
    pub fn new() -> Self {
        // O padrão de `cycle` é `true` no original, apesar do comentário da interface dizer `false`.
        let defaults = InternalParameterHintOptions { enabled: true, cycle: true };
        let schema = Dynamic::object([
            (
                "editor.parameterHints.enabled",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", defaults.enabled.into()),
                    ("description", Dynamic::string("Enables a pop-up that shows parameter documentation and type information as you type.")),
                ]),
            ),
            (
                "editor.parameterHints.cycle",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", defaults.cycle.into()),
                    ("description", Dynamic::string("Controls whether the parameter hints menu cycles or closes when reaching the end of the list.")),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorParameterHints {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorParameterHints {
    type Value = InternalParameterHintOptions;

    fn id(&self) -> EditorOption {
        EditorOption::ParameterHints
    }

    fn name(&self) -> &str {
        "parameterHints"
    }

    fn default_value(&self) -> InternalParameterHintOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> InternalParameterHintOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        InternalParameterHintOptions {
            enabled: boolean(&input.get_str("enabled"), self.default_value.enabled),
            cycle: boolean(&input.get_str("cycle"), self.default_value.cycle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter_hints(enabled: Dynamic, cycle: Dynamic) -> InternalParameterHintOptions {
        EditorParameterHints::new().validate(&Dynamic::object([("enabled", enabled), ("cycle", cycle)]))
    }

    #[test]
    fn both_flags_default_to_true() {
        let value = EditorParameterHints::new().default_value();
        assert!(value.enabled && value.cycle);
    }

    #[test]
    fn non_objects_and_empty_objects_give_the_default() {
        let option = EditorParameterHints::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(false), Dynamic::Number(0.0), Dynamic::string("false")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        assert_eq!(option.validate(&Dynamic::object::<&str>([])), option.default_value());
    }

    #[test]
    fn flags_are_independent() {
        let off = Dynamic::Bool(false);
        let none = Dynamic::Undefined;
        assert_eq!(parameter_hints(off.clone(), none.clone()), InternalParameterHintOptions { enabled: false, cycle: true });
        assert_eq!(parameter_hints(none, off.clone()), InternalParameterHintOptions { enabled: true, cycle: false });
        assert_eq!(parameter_hints(off.clone(), off), InternalParameterHintOptions { enabled: false, cycle: false });
        // A string 'false' conta como falso.
        assert!(!parameter_hints(Dynamic::string("false"), Dynamic::Undefined).enabled);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = EditorParameterHints::new();
        let schema = option.schema().expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["editor.parameterHints.enabled", "editor.parameterHints.cycle"]);
        let cycle = schema.get_str("editor.parameterHints.cycle");
        assert_eq!(cycle.own_keys(), vec!["type", "default", "description"]);
        assert_eq!(cycle.get_str("default"), Dynamic::Bool(true));
    }
}
