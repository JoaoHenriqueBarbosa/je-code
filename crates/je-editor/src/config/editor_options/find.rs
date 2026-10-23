//! Porte de `EditorFind` (`editorOptions.ts`, região `find`).

use je_base::dynamic::{Dynamic, Truthy};
use je_base::platform::Platform;

use crate::config::editor_options::option_support::string_enum;
use crate::config::editor_options::{boolean, strings_array, EditorOption, EditorOptionImpl, Opt};

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

/// `EditorFindOptions` (`Readonly<Required<IEditorFindOptions>>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorFindOptions {
    /// O cursor deve ir para as ocorrências enquanto se digita.
    pub cursor_move_on_type: bool,
    /// O widget busca enquanto se digita.
    pub find_on_type: bool,
    pub seed_search_string_from_selection: SeedSearchStringFromSelection,
    pub auto_find_in_selection: AutoFindInSelection,
    /// Interno: o widget lê e altera a área de transferência de busca compartilhada do macOS.
    pub global_find_clipboard: bool,
    /// O widget acrescenta linhas extras no alto do editor.
    pub add_extra_space_on_top: bool,
    /// A busca recomeça do início (ou do fim) quando não há mais ocorrências (`loop`).
    pub r#loop: bool,
    /// O widget fecha depois que um comando de navegação explícito cai numa ocorrência.
    pub close_on_result: bool,
    /// Interno: como guardar o histórico de busca.
    pub history: FindHistory,
    /// Interno: como guardar o histórico de substituição.
    pub replace_history: FindHistory,
}

/// Chave tipada da opção `find`.
pub const FIND: Opt<EditorFindOptions> = Opt::new(EditorOption::Find);

/// `{ type: 'boolean', default, description }`.
fn boolean_entry(default: bool, description: &str) -> Vec<(&'static str, Dynamic)> {
    vec![
        ("type", Dynamic::string("boolean")),
        ("default", default.into()),
        ("description", Dynamic::string(description)),
    ]
}

/// `{ type: 'string', enum, default, enumDescriptions, description }`.
fn enum_entry(allowed: &[&str], default: &str, enum_descriptions: &[&str], description: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string("string")),
        ("enum", strings_array(allowed)),
        ("default", Dynamic::string(default)),
        ("enumDescriptions", strings_array(enum_descriptions)),
        ("description", Dynamic::string(description)),
    ])
}

/// `EditorFind`.
pub struct EditorFind {
    default_value: EditorFindOptions,
    schema: Dynamic,
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
        let boolean_schema = |default: bool, description: &str| Dynamic::object(boolean_entry(default, description));
        let schema = Dynamic::object([
            (
                "editor.find.cursorMoveOnType",
                boolean_schema(defaults.cursor_move_on_type, "Controls whether the cursor should jump to find matches while typing."),
            ),
            (
                "editor.find.seedSearchStringFromSelection",
                enum_entry(
                    SeedSearchStringFromSelection::ALLOWED,
                    defaults.seed_search_string_from_selection.as_str(),
                    &[
                        "Never seed search string from the editor selection.",
                        "Always seed search string from the editor selection, including word at cursor position.",
                        "Only seed search string from the editor selection.",
                    ],
                    "Controls whether the search string in the Find Widget is seeded from the editor selection.",
                ),
            ),
            (
                "editor.find.autoFindInSelection",
                enum_entry(
                    AutoFindInSelection::ALLOWED,
                    defaults.auto_find_in_selection.as_str(),
                    &[
                        "Never turn on Find in Selection automatically (default).",
                        "Always turn on Find in Selection automatically.",
                        "Turn on Find in Selection automatically when multiple lines of content are selected.",
                    ],
                    "Controls the condition for turning on Find in Selection automatically.",
                ),
            ),
            (
                "editor.find.globalFindClipboard",
                {
                    // `included: platform.isMacintosh`: o registro de configuração ignora a chave fora do macOS.
                    let mut entries = boolean_entry(
                        defaults.global_find_clipboard,
                        "Controls whether the Find Widget should read or modify the shared find clipboard on macOS.",
                    );
                    entries.push(("included", Dynamic::Bool(Platform::detect().is_macintosh)));
                    Dynamic::object(entries)
                },
            ),
            (
                "editor.find.addExtraSpaceOnTop",
                boolean_schema(
                    defaults.add_extra_space_on_top,
                    "Controls whether the Find Widget should add extra lines on top of the editor. When true, you can scroll beyond the first line when the Find Widget is visible.",
                ),
            ),
            (
                "editor.find.loop",
                boolean_schema(
                    defaults.r#loop,
                    "Controls whether the search automatically restarts from the beginning (or the end) when no further matches can be found.",
                ),
            ),
            (
                "editor.find.closeOnResult",
                boolean_schema(
                    defaults.close_on_result,
                    "Controls whether the Find Widget closes after an explicit find navigation command lands on a result.",
                ),
            ),
            (
                "editor.find.history",
                enum_entry(
                    FindHistory::ALLOWED,
                    "workspace",
                    &["Do not store search history from the find widget.", "Store search history across the active workspace"],
                    "Controls how the find widget history should be stored",
                ),
            ),
            (
                "editor.find.replaceHistory",
                enum_entry(
                    FindHistory::ALLOWED,
                    "workspace",
                    &["Do not store history from the replace widget.", "Store replace history across the active workspace"],
                    "Controls how the replace widget history should be stored",
                ),
            ),
            ("editor.find.findOnType", boolean_schema(defaults.find_on_type, "Controls whether the Find Widget should search as you type.")),
        ]);
        Self { default_value: defaults, schema }
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

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> EditorFindOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value;
        }
        let defaults = &self.default_value;
        // `typeof x === 'boolean'`: o booleano legado vira `always` ou `never`.
        let seed_search_string_from_selection = match input.get_str("seedSearchStringFromSelection") {
            Dynamic::Bool(true) => SeedSearchStringFromSelection::Always,
            Dynamic::Bool(false) => SeedSearchStringFromSelection::Never,
            other => SeedSearchStringFromSelection::validate(&other, defaults.seed_search_string_from_selection),
        };
        let auto_find_in_selection = match input.get_str("autoFindInSelection") {
            Dynamic::Bool(true) => AutoFindInSelection::Always,
            Dynamic::Bool(false) => AutoFindInSelection::Never,
            other => AutoFindInSelection::validate(&other, defaults.auto_find_in_selection),
        };
        EditorFindOptions {
            cursor_move_on_type: boolean(&input.get_str("cursorMoveOnType"), defaults.cursor_move_on_type),
            find_on_type: boolean(&input.get_str("findOnType"), defaults.find_on_type),
            seed_search_string_from_selection,
            auto_find_in_selection,
            global_find_clipboard: boolean(&input.get_str("globalFindClipboard"), defaults.global_find_clipboard),
            add_extra_space_on_top: boolean(&input.get_str("addExtraSpaceOnTop"), defaults.add_extra_space_on_top),
            r#loop: boolean(&input.get_str("loop"), defaults.r#loop),
            close_on_result: boolean(&input.get_str("closeOnResult"), defaults.close_on_result),
            history: FindHistory::validate(&input.get_str("history"), defaults.history),
            replace_history: FindHistory::validate(&input.get_str("replaceHistory"), defaults.replace_history),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Dynamic {
        Dynamic::string(value)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> EditorFindOptions {
        EditorOptionImpl::validate(&EditorFind::new(), &Dynamic::object(entries))
    }

    fn defaults() -> EditorFindOptions {
        EditorOptionImpl::default_value(&EditorFind::new())
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = defaults();
        assert!(defaults.cursor_move_on_type && defaults.find_on_type && defaults.add_extra_space_on_top && defaults.r#loop);
        assert!(!defaults.global_find_clipboard && !defaults.close_on_result);
        assert_eq!(defaults.seed_search_string_from_selection, SeedSearchStringFromSelection::Always);
        assert_eq!(defaults.auto_find_in_selection, AutoFindInSelection::Never);
        assert_eq!(defaults.history, FindHistory::Workspace);
        assert_eq!(defaults.replace_history, FindHistory::Workspace);
        assert_eq!(validate(vec![]), defaults);
        let option = EditorFind::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(1.0), text("find")] {
            assert_eq!(EditorOptionImpl::validate(&option, &input), defaults);
        }
    }

    #[test]
    fn legacy_boolean_seed_and_auto_find() {
        assert_eq!(
            validate(vec![("seedSearchStringFromSelection", Dynamic::Bool(true))]).seed_search_string_from_selection,
            SeedSearchStringFromSelection::Always
        );
        assert_eq!(
            validate(vec![("seedSearchStringFromSelection", Dynamic::Bool(false))]).seed_search_string_from_selection,
            SeedSearchStringFromSelection::Never
        );
        assert_eq!(validate(vec![("autoFindInSelection", Dynamic::Bool(true))]).auto_find_in_selection, AutoFindInSelection::Always);
        assert_eq!(validate(vec![("autoFindInSelection", Dynamic::Bool(false))]).auto_find_in_selection, AutoFindInSelection::Never);
    }

    #[test]
    fn string_values_and_invalid_values() {
        assert_eq!(
            validate(vec![("seedSearchStringFromSelection", text("selection"))]).seed_search_string_from_selection,
            SeedSearchStringFromSelection::Selection
        );
        assert_eq!(validate(vec![("autoFindInSelection", text("multiline"))]).auto_find_in_selection, AutoFindInSelection::Multiline);

        // Valor inválido (outra string, número, `null`) cai no padrão.
        let invalid = validate(vec![
            ("seedSearchStringFromSelection", text("sometimes")),
            ("autoFindInSelection", Dynamic::Number(1.0)),
            ("history", text("forever")),
            ("replaceHistory", text("")),
        ]);
        assert_eq!(invalid, defaults());
        assert_eq!(validate(vec![("autoFindInSelection", Dynamic::Null)]).auto_find_in_selection, AutoFindInSelection::Never);
        // `multiline` não vale para `seedSearchStringFromSelection`.
        assert_eq!(
            validate(vec![("seedSearchStringFromSelection", text("multiline"))]).seed_search_string_from_selection,
            SeedSearchStringFromSelection::Always
        );
    }

    #[test]
    fn booleans_and_history() {
        let value = validate(vec![
            ("cursorMoveOnType", Dynamic::Bool(false)),
            ("findOnType", text("false")),
            ("addExtraSpaceOnTop", Dynamic::Number(0.0)),
            ("globalFindClipboard", Dynamic::Bool(true)),
            ("loop", Dynamic::Bool(false)),
            ("closeOnResult", Dynamic::Bool(true)),
            ("history", text("never")),
            ("replaceHistory", text("never")),
        ]);
        assert!(!value.cursor_move_on_type && !value.find_on_type && !value.add_extra_space_on_top && !value.r#loop);
        assert!(value.global_find_clipboard && value.close_on_result);
        assert_eq!(value.history, FindHistory::Never);
        assert_eq!(value.replace_history, FindHistory::Never);
    }

    #[test]
    fn schema_keys_order_and_entries() {
        let option = EditorFind::new();
        let schema = EditorOptionImpl::schema(&option).expect("sem esquema");
        assert_eq!(
            schema.own_keys(),
            vec![
                "editor.find.cursorMoveOnType",
                "editor.find.seedSearchStringFromSelection",
                "editor.find.autoFindInSelection",
                "editor.find.globalFindClipboard",
                "editor.find.addExtraSpaceOnTop",
                "editor.find.loop",
                "editor.find.closeOnResult",
                "editor.find.history",
                "editor.find.replaceHistory",
                "editor.find.findOnType",
            ]
        );
        let clipboard = schema.get_str("editor.find.globalFindClipboard");
        assert_eq!(clipboard.own_keys(), vec!["type", "default", "description", "included"]);
        assert_eq!(clipboard.get_str("included"), Dynamic::Bool(Platform::detect().is_macintosh));
        let seed = schema.get_str("editor.find.seedSearchStringFromSelection");
        assert_eq!(seed.own_keys(), vec!["type", "enum", "default", "enumDescriptions", "description"]);
        assert_eq!(seed.get_str("enum").get_str("length"), Dynamic::Number(3.0));
        assert_eq!(seed.get_str("default"), text("always"));
        let history = schema.get_str("editor.find.history");
        assert_eq!(history.get_str("enum").get_str("length"), Dynamic::Number(2.0));
        assert_eq!(history.get_str("enumDescriptions").get_str("length"), Dynamic::Number(2.0));
    }
}
