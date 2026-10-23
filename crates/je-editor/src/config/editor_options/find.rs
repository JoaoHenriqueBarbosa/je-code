//! Porte de `EditorFind` (`editorOptions.ts`, região `find`).
//!
//! Divergências:
//! - `validate(unknown)` do original lê os campos do objeto. [`OptionInput`] ainda não carrega objetos,
//!   então [`EditorOptionImpl::validate`] devolve o padrão e a validação real é
//!   [`EditorFind::validate_input`]. `// TODO(config)`: ligar quando `OptionInput` virar `Dynamic`.
//! - `seedSearchStringFromSelection` e `autoFindInSelection` aceitam, em tempo de execução, o booleano
//!   legado (`true` é `always`, `false` é `never`), por isso os campos de [`IEditorFindOptions`] são
//!   [`OptionInput`] nesses dois casos.
//! - `included: platform.isMacintosh` de `editor.find.globalFindClipboard`: a chave só entra no esquema no
//!   macOS, que é o efeito de `included: false` no registro de configuração.
//! - `enumDescriptions` ainda não existe em `ConfigurationPropertySchema`. `// TODO(config)`.

use je_base::platform::Platform;

use crate::config::editor_options::option_support::{boolean_schema, string_enum, string_enum_schema, string_input};
use crate::config::editor_options::{EditorOption, EditorOptionImpl, Opt, OptionInput, OptionSchema};

string_enum! {
    /// `'never' | 'always' | 'selection'`.
    pub enum SeedSearchStringFromSelection {
        Never => "never",
        Always => "always",
        Selection => "selection",
    }
}

string_enum! {
    /// `'never' | 'always' | 'multiline'`.
    pub enum AutoFindInSelection {
        Never => "never",
        Always => "always",
        Multiline => "multiline",
    }
}

string_enum! {
    /// `'never' | 'workspace'`.
    pub enum FindHistory {
        Never => "never",
        Workspace => "workspace",
    }
}

/// `IEditorFindOptions`: configuração do widget de busca.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IEditorFindOptions {
    /// O cursor deve ir para as ocorrências enquanto se digita.
    pub cursor_move_on_type: Option<bool>,
    /// O widget busca enquanto se digita.
    pub find_on_type: Option<bool>,
    /// `'never' | 'always' | 'selection'`, ou o booleano legado.
    pub seed_search_string_from_selection: Option<OptionInput>,
    /// `'never' | 'always' | 'multiline'`, ou o booleano legado.
    pub auto_find_in_selection: Option<OptionInput>,
    /// O widget acrescenta linhas extras no alto do editor.
    pub add_extra_space_on_top: Option<bool>,
    /// Interno: o widget lê e altera a área de transferência de busca compartilhada do macOS.
    pub global_find_clipboard: Option<bool>,
    /// A busca recomeça do início (ou do fim) quando não há mais ocorrências (`loop`).
    pub r#loop: Option<bool>,
    /// O widget fecha depois que um comando de navegação explícito cai numa ocorrência.
    pub close_on_result: Option<bool>,
    /// Interno: `'never' | 'workspace'`, como guardar o histórico de busca.
    pub history: Option<String>,
    /// Interno: `'never' | 'workspace'`, como guardar o histórico de substituição.
    pub replace_history: Option<String>,
}

/// `EditorFindOptions` (`Readonly<Required<IEditorFindOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorFindOptions {
    pub cursor_move_on_type: bool,
    pub find_on_type: bool,
    pub seed_search_string_from_selection: SeedSearchStringFromSelection,
    pub auto_find_in_selection: AutoFindInSelection,
    pub global_find_clipboard: bool,
    pub add_extra_space_on_top: bool,
    pub r#loop: bool,
    pub close_on_result: bool,
    pub history: FindHistory,
    pub replace_history: FindHistory,
}

/// Chave tipada da opção `find`.
pub const FIND: Opt<EditorFindOptions> = Opt::new(EditorOption::Find);

/// `EditorFind`.
pub struct EditorFind {
    default_value: EditorFindOptions,
    schema: OptionSchema,
}

impl EditorFind {
    pub fn new() -> Self {
        let defaults = EditorFindOptions {
            cursor_move_on_type: true,
            find_on_type: true,
            seed_search_string_from_selection: SeedSearchStringFromSelection::Always,
            auto_find_in_selection: AutoFindInSelection::Never,
            global_find_clipboard: false,
            add_extra_space_on_top: true,
            r#loop: true,
            close_on_result: false,
            history: FindHistory::Workspace,
            replace_history: FindHistory::Workspace,
        };
        let mut entries = vec![
            (
                "editor.find.cursorMoveOnType".to_string(),
                boolean_schema(defaults.cursor_move_on_type, "Controls whether the cursor should jump to find matches while typing."),
            ),
            (
                "editor.find.seedSearchStringFromSelection".to_string(),
                string_enum_schema(
                    defaults.seed_search_string_from_selection.as_str(),
                    SeedSearchStringFromSelection::ALLOWED,
                    "Controls whether the search string in the Find Widget is seeded from the editor selection.",
                ),
            ),
            (
                "editor.find.autoFindInSelection".to_string(),
                string_enum_schema(
                    defaults.auto_find_in_selection.as_str(),
                    AutoFindInSelection::ALLOWED,
                    "Controls the condition for turning on Find in Selection automatically.",
                ),
            ),
        ];
        if Platform::detect().is_macintosh {
            entries.push((
                "editor.find.globalFindClipboard".to_string(),
                boolean_schema(
                    defaults.global_find_clipboard,
                    "Controls whether the Find Widget should read or modify the shared find clipboard on macOS.",
                ),
            ));
        }
        entries.extend([
            (
                "editor.find.addExtraSpaceOnTop".to_string(),
                boolean_schema(
                    defaults.add_extra_space_on_top,
                    "Controls whether the Find Widget should add extra lines on top of the editor. When true, you can scroll beyond the first line when the Find Widget is visible.",
                ),
            ),
            (
                "editor.find.loop".to_string(),
                boolean_schema(
                    defaults.r#loop,
                    "Controls whether the search automatically restarts from the beginning (or the end) when no further matches can be found.",
                ),
            ),
            (
                "editor.find.closeOnResult".to_string(),
                boolean_schema(
                    defaults.close_on_result,
                    "Controls whether the Find Widget closes after an explicit find navigation command lands on a result.",
                ),
            ),
            (
                "editor.find.history".to_string(),
                string_enum_schema(
                    defaults.history.as_str(),
                    FindHistory::ALLOWED,
                    "Controls how the find widget history should be stored",
                ),
            ),
            (
                "editor.find.replaceHistory".to_string(),
                string_enum_schema(
                    defaults.replace_history.as_str(),
                    FindHistory::ALLOWED,
                    "Controls how the replace widget history should be stored",
                ),
            ),
            (
                "editor.find.findOnType".to_string(),
                boolean_schema(defaults.find_on_type, "Controls whether the Find Widget should search as you type."),
            ),
        ]);
        Self { default_value: defaults, schema: OptionSchema::Map(entries) }
    }

    /// `validate` sobre a entrada tipada. `None` é a entrada que não é objeto (`undefined`, `null`...).
    pub fn validate_input(&self, input: Option<&IEditorFindOptions>) -> EditorFindOptions {
        let Some(input) = input else {
            return self.default_value;
        };
        let defaults = &self.default_value;
        let undefined = OptionInput::Undefined;
        // `typeof x === 'boolean'`: o booleano legado vira `always` ou `never`.
        let seed_search_string_from_selection = match &input.seed_search_string_from_selection {
            Some(OptionInput::Bool(true)) => SeedSearchStringFromSelection::Always,
            Some(OptionInput::Bool(false)) => SeedSearchStringFromSelection::Never,
            other => SeedSearchStringFromSelection::validate(other.as_ref().unwrap_or(&undefined), defaults.seed_search_string_from_selection),
        };
        let auto_find_in_selection = match &input.auto_find_in_selection {
            Some(OptionInput::Bool(true)) => AutoFindInSelection::Always,
            Some(OptionInput::Bool(false)) => AutoFindInSelection::Never,
            other => AutoFindInSelection::validate(other.as_ref().unwrap_or(&undefined), defaults.auto_find_in_selection),
        };
        EditorFindOptions {
            cursor_move_on_type: input.cursor_move_on_type.unwrap_or(defaults.cursor_move_on_type),
            find_on_type: input.find_on_type.unwrap_or(defaults.find_on_type),
            seed_search_string_from_selection,
            auto_find_in_selection,
            global_find_clipboard: input.global_find_clipboard.unwrap_or(defaults.global_find_clipboard),
            add_extra_space_on_top: input.add_extra_space_on_top.unwrap_or(defaults.add_extra_space_on_top),
            r#loop: input.r#loop.unwrap_or(defaults.r#loop),
            close_on_result: input.close_on_result.unwrap_or(defaults.close_on_result),
            history: FindHistory::validate(&string_input(input.history.as_deref()), defaults.history),
            replace_history: FindHistory::validate(&string_input(input.replace_history.as_deref()), defaults.replace_history),
        }
    }
}

impl Default for EditorFind {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorFind {
    type Value = EditorFindOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Find
    }

    fn name(&self) -> &str {
        "find"
    }

    fn default_value(&self) -> EditorFindOptions {
        self.default_value
    }

    fn schema(&self) -> Option<&OptionSchema> {
        Some(&self.schema)
    }

    fn validate(&self, _input: &OptionInput) -> EditorFindOptions {
        // `// TODO(config)`: ver a divergência no topo do arquivo.
        self.default_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_with(tweak: impl FnOnce(&mut IEditorFindOptions)) -> EditorFindOptions {
        let mut input = IEditorFindOptions::default();
        tweak(&mut input);
        EditorFind::new().validate_input(Some(&input))
    }

    #[test]
    fn defaults_match_the_original() {
        let option = EditorFind::new();
        let defaults = EditorOptionImpl::default_value(&option);
        assert!(defaults.cursor_move_on_type && defaults.find_on_type && defaults.add_extra_space_on_top && defaults.r#loop);
        assert!(!defaults.global_find_clipboard && !defaults.close_on_result);
        assert_eq!(defaults.seed_search_string_from_selection, SeedSearchStringFromSelection::Always);
        assert_eq!(defaults.auto_find_in_selection, AutoFindInSelection::Never);
        assert_eq!(defaults.history, FindHistory::Workspace);
        assert_eq!(defaults.replace_history, FindHistory::Workspace);
        assert_eq!(option.validate_input(None), defaults);
        assert_eq!(option.validate_input(Some(&IEditorFindOptions::default())), defaults);
        assert_eq!(EditorOptionImpl::validate(&option, &OptionInput::Bool(true)), defaults);
    }

    #[test]
    fn legacy_boolean_seed_and_auto_find() {
        let seed_true = input_with(|i| i.seed_search_string_from_selection = Some(OptionInput::Bool(true)));
        assert_eq!(seed_true.seed_search_string_from_selection, SeedSearchStringFromSelection::Always);
        let seed_false = input_with(|i| i.seed_search_string_from_selection = Some(OptionInput::Bool(false)));
        assert_eq!(seed_false.seed_search_string_from_selection, SeedSearchStringFromSelection::Never);
        let auto_true = input_with(|i| i.auto_find_in_selection = Some(OptionInput::Bool(true)));
        assert_eq!(auto_true.auto_find_in_selection, AutoFindInSelection::Always);
        let auto_false = input_with(|i| i.auto_find_in_selection = Some(OptionInput::Bool(false)));
        assert_eq!(auto_false.auto_find_in_selection, AutoFindInSelection::Never);
    }

    #[test]
    fn string_values_and_invalid_values() {
        let selection = input_with(|i| i.seed_search_string_from_selection = Some(OptionInput::string("selection")));
        assert_eq!(selection.seed_search_string_from_selection, SeedSearchStringFromSelection::Selection);
        let multiline = input_with(|i| i.auto_find_in_selection = Some(OptionInput::string("multiline")));
        assert_eq!(multiline.auto_find_in_selection, AutoFindInSelection::Multiline);

        // Valor inválido (outra string, número, `null`) cai no padrão.
        let invalid = input_with(|i| {
            i.seed_search_string_from_selection = Some(OptionInput::string("sometimes"));
            i.auto_find_in_selection = Some(OptionInput::Number(1.0));
            i.history = Some("forever".to_string());
            i.replace_history = Some(String::new());
        });
        assert_eq!(invalid.seed_search_string_from_selection, SeedSearchStringFromSelection::Always);
        assert_eq!(invalid.auto_find_in_selection, AutoFindInSelection::Never);
        assert_eq!(invalid.history, FindHistory::Workspace);
        assert_eq!(invalid.replace_history, FindHistory::Workspace);
        let null = input_with(|i| i.auto_find_in_selection = Some(OptionInput::Null));
        assert_eq!(null.auto_find_in_selection, AutoFindInSelection::Never);
        // `multiline` não vale para `seedSearchStringFromSelection`.
        let crossed = input_with(|i| i.seed_search_string_from_selection = Some(OptionInput::string("multiline")));
        assert_eq!(crossed.seed_search_string_from_selection, SeedSearchStringFromSelection::Always);
    }

    #[test]
    fn booleans_and_history() {
        let value = input_with(|i| {
            i.cursor_move_on_type = Some(false);
            i.find_on_type = Some(false);
            i.add_extra_space_on_top = Some(false);
            i.global_find_clipboard = Some(true);
            i.r#loop = Some(false);
            i.close_on_result = Some(true);
            i.history = Some("never".to_string());
            i.replace_history = Some("never".to_string());
        });
        assert!(!value.cursor_move_on_type && !value.find_on_type && !value.add_extra_space_on_top && !value.r#loop);
        assert!(value.global_find_clipboard && value.close_on_result);
        assert_eq!(value.history, FindHistory::Never);
        assert_eq!(value.replace_history, FindHistory::Never);
    }

    #[test]
    fn schema_keys_and_enums() {
        let option = EditorFind::new();
        let Some(OptionSchema::Map(entries)) = EditorOptionImpl::schema(&option) else { panic!("sem esquema") };
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        let has_clipboard = keys.contains(&"editor.find.globalFindClipboard");
        assert_eq!(has_clipboard, Platform::detect().is_macintosh);
        assert_eq!(keys.len(), if has_clipboard { 10 } else { 9 });
        assert_eq!(keys.first(), Some(&"editor.find.cursorMoveOnType"));
        assert_eq!(keys.last(), Some(&"editor.find.findOnType"));
        let (_, seed) = entries.iter().find(|(key, _)| key == "editor.find.seedSearchStringFromSelection").unwrap();
        assert_eq!(seed.enum_values.as_deref(), Some(&["never".to_string(), "always".to_string(), "selection".to_string()][..]));
        let (_, history) = entries.iter().find(|(key, _)| key == "editor.find.history").unwrap();
        assert_eq!(history.enum_values.as_ref().map(Vec::len), Some(2));
    }
}
