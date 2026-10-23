//! Porte de `EditorComments` (`editorOptions.ts`, região `comments`).
//!
//! Divergência: `validate(unknown)` do original lê os campos do objeto. [`OptionInput`] ainda não
//! carrega objetos (`Other` é opaco), então [`EditorOptionImpl::validate`] devolve o padrão e a validação
//! real é [`EditorComments::validate_input`], sobre a struct tipada. `// TODO(config)`: quando
//! `OptionInput` virar `Dynamic`, `validate` lê os campos e chama `validate_input`.

use crate::config::editor_options::option_support::boolean_schema;
use crate::config::editor_options::{EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema};

/// `IEditorCommentsOptions`: configuração dos comentários do editor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IEditorCommentsOptions {
    /// Insere um espaço depois do token de comentário de linha e dentro dos tokens de bloco. Padrão: `true`.
    pub insert_space: Option<bool>,
    /// Ignora linhas vazias ao inserir comentários de linha. Padrão: `true`.
    pub ignore_empty_lines: Option<bool>,
}

/// `EditorCommentsOptions` (`Readonly<Required<IEditorCommentsOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorCommentsOptions {
    pub insert_space: bool,
    pub ignore_empty_lines: bool,
}

/// Chave tipada da opção `comments`.
pub const COMMENTS: Opt<EditorCommentsOptions> = Opt::new(EditorOption::Comments);

/// `EditorComments`.
pub struct EditorComments {
    default_value: EditorCommentsOptions,
    schema: OptionSchema,
}

impl EditorComments {
    pub fn new() -> Self {
        let defaults = EditorCommentsOptions { insert_space: true, ignore_empty_lines: true };
        let schema = OptionSchema::Map(vec![
            (
                "editor.comments.insertSpace".to_string(),
                boolean_schema(defaults.insert_space, "Controls whether a space character is inserted when commenting."),
            ),
            (
                "editor.comments.ignoreEmptyLines".to_string(),
                boolean_schema(
                    defaults.ignore_empty_lines,
                    "Controls if empty lines should be ignored with toggle, add or remove actions for line comments.",
                ),
            ),
        ]);
        Self { default_value: defaults, schema }
    }

    /// `validate` sobre a entrada tipada. `None` é a entrada que não é objeto (`undefined`, `null`...).
    pub fn validate_input(&self, input: Option<&IEditorCommentsOptions>) -> EditorCommentsOptions {
        let Some(input) = input else {
            return self.default_value;
        };
        EditorCommentsOptions {
            insert_space: input.insert_space.unwrap_or(self.default_value.insert_space),
            ignore_empty_lines: input.ignore_empty_lines.unwrap_or(self.default_value.ignore_empty_lines),
        }
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

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, _input: &OptionInput) -> EditorCommentsOptions {
        // `// TODO(config)`: ver a divergência no topo do arquivo.
        self.default_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_non_object_input() {
        let option = EditorComments::new();
        let defaults = EditorCommentsOptions { insert_space: true, ignore_empty_lines: true };
        assert_eq!(EditorOptionImpl::default_value(&option), defaults);
        assert_eq!(option.validate_input(None), defaults);
        assert_eq!(EditorOptionImpl::validate(&option, &OptionInput::Number(3.0)), defaults);
        assert_eq!(EditorOptionImpl::name(&option), "comments");
        assert_eq!(EditorOptionImpl::id(&option), EditorOption::Comments);
    }

    #[test]
    fn missing_fields_fall_back_to_the_default() {
        let option = EditorComments::new();
        let input = IEditorCommentsOptions { insert_space: Some(false), ignore_empty_lines: None };
        assert_eq!(option.validate_input(Some(&input)), EditorCommentsOptions { insert_space: false, ignore_empty_lines: true });
        let input = IEditorCommentsOptions { insert_space: None, ignore_empty_lines: Some(false) };
        assert_eq!(option.validate_input(Some(&input)), EditorCommentsOptions { insert_space: true, ignore_empty_lines: false });
        assert!(option.validate_input(Some(&IEditorCommentsOptions::default())).insert_space);
    }

    #[test]
    fn schema_has_one_boolean_entry_per_key() {
        let option = EditorComments::new();
        let Some(OptionSchema::Map(entries)) = EditorOptionImpl::schema(&option) else { panic!("sem esquema") };
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys, ["editor.comments.insertSpace", "editor.comments.ignoreEmptyLines"]);
        assert!(entries.iter().all(|(_, schema)| schema.type_.as_deref() == Some("boolean")));
    }
}
