//! Porte de `EditorComments` (`editorOptions.ts`, região `comments`).

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, EditorOption, EditorOptionImpl, Opt};

/// `EditorCommentsOptions` (`Readonly<Required<IEditorCommentsOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorCommentsOptions {
    /// Insere um espaço depois do token de comentário de linha e dentro dos tokens de bloco.
    pub insert_space: bool,
    /// Ignora linhas vazias ao inserir comentários de linha.
    pub ignore_empty_lines: bool,
}

/// Chave tipada da opção `comments`.
pub const COMMENTS: Opt<EditorCommentsOptions> = Opt::new(EditorOption::Comments);

/// `EditorComments`.
pub struct EditorComments {
    default_value: EditorCommentsOptions,
    schema: Dynamic,
}

impl EditorComments {
    pub fn new() -> Self {
        let defaults = EditorCommentsOptions { insert_space: true, ignore_empty_lines: true };
        let schema = Dynamic::object([
            (
                "editor.comments.insertSpace",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", defaults.insert_space.into()),
                    ("description", Dynamic::string("Controls whether a space character is inserted when commenting.")),
                ]),
            ),
            (
                "editor.comments.ignoreEmptyLines",
                Dynamic::object([
                    ("type", Dynamic::string("boolean")),
                    ("default", defaults.ignore_empty_lines.into()),
                    (
                        "description",
                        Dynamic::string("Controls if empty lines should be ignored with toggle, add or remove actions for line comments."),
                    ),
                ]),
            ),
        ]);
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorComments {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorComments {
    type Value = EditorCommentsOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Comments
    }

    fn name(&self) -> &str {
        "comments"
    }

    fn default_value(&self) -> EditorCommentsOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorCommentsOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        EditorCommentsOptions {
            insert_space: boolean(&input.get_str("insertSpace"), self.default_value.insert_space),
            ignore_empty_lines: boolean(&input.get_str("ignoreEmptyLines"), self.default_value.ignore_empty_lines),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: EditorCommentsOptions = EditorCommentsOptions { insert_space: true, ignore_empty_lines: true };

    fn validate(input: &Dynamic) -> EditorCommentsOptions {
        EditorOptionImpl::validate(&EditorComments::new(), input)
    }

    #[test]
    fn non_object_input_is_the_default() {
        let option = EditorComments::new();
        assert_eq!(EditorOptionImpl::default_value(&option), DEFAULTS);
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(3.0), Dynamic::string("x"), Dynamic::string("")] {
            assert_eq!(validate(&input), DEFAULTS);
        }
        assert_eq!(EditorOptionImpl::name(&option), "comments");
        assert_eq!(EditorOptionImpl::id(&option), EditorOption::Comments);
    }

    #[test]
    fn missing_fields_fall_back_to_the_default() {
        assert_eq!(validate(&Dynamic::object::<&str>([])), DEFAULTS);
        let input = Dynamic::object([("insertSpace", Dynamic::Bool(false))]);
        assert_eq!(validate(&input), EditorCommentsOptions { insert_space: false, ignore_empty_lines: true });
        let input = Dynamic::object([("ignoreEmptyLines", Dynamic::Bool(false))]);
        assert_eq!(validate(&input), EditorCommentsOptions { insert_space: true, ignore_empty_lines: false });
    }

    #[test]
    fn fields_use_the_boolean_coercion() {
        let input = Dynamic::object([("insertSpace", Dynamic::string("false")), ("ignoreEmptyLines", Dynamic::Number(0.0))]);
        assert_eq!(validate(&input), EditorCommentsOptions { insert_space: false, ignore_empty_lines: false });
        let input = Dynamic::object([("insertSpace", Dynamic::Null), ("ignoreEmptyLines", Dynamic::string("x"))]);
        assert_eq!(validate(&input), EditorCommentsOptions { insert_space: false, ignore_empty_lines: true });
        // Um array também é objeto, mas não tem os campos.
        assert_eq!(validate(&Dynamic::array([])), DEFAULTS);
    }

    #[test]
    fn schema_has_one_boolean_entry_per_key_in_order() {
        let option = EditorComments::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(schema.own_keys(), vec!["editor.comments.insertSpace", "editor.comments.ignoreEmptyLines"]);
        let entry = schema.get_str("editor.comments.insertSpace");
        assert_eq!(entry.own_keys(), vec!["type", "default", "description"]);
        assert_eq!(entry.get_str("type"), Dynamic::string("boolean"));
        assert_eq!(entry.get_str("default"), Dynamic::Bool(true));
    }
}
