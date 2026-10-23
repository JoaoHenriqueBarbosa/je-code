//! Porte de `EditorSuggest`, `ISuggestOptions` e `InternalSuggestOptions` (`editorOptions.ts`, linhas 4991
//! a 5486).
//!
//! Divergências e fidelidades:
//!
//! - A entrada (`ISuggestOptions`) é um objeto [`Dynamic`], como nas demais opções.
//! - `snippetsPreventQuickSuggestions` usa o padrão de `filterGraceful` quando o campo falta, como o
//!   original (`this.defaultValue.filterGraceful`).
//! - O esquema não tem `previewMode` nem `fitWidthToDetails`, e tem `showCustomcolors`, que não é campo
//!   do valor: tudo como no original.

use je_base::dynamic::{Dynamic, Truthy};

use crate::config::editor_options::{boolean, strings_array, EditorOption, EditorOptionImpl, Opt};

/// Chave tipada de `suggest`.
pub const SUGGEST: Opt<InternalSuggestOptions> = Opt::new(EditorOption::Suggest);

string_enum! {
    /// `'insert' | 'replace'`.
    pub enum SuggestInsertMode {
        Insert => "insert",
        Replace => "replace",
    }
}

string_enum! {
    /// `'always' | 'never' | 'whenTriggerCharacter' | 'whenQuickSuggestion'`.
    pub enum SuggestSelectionMode {
        Always => "always",
        Never => "never",
        WhenTriggerCharacter => "whenTriggerCharacter",
        WhenQuickSuggestion => "whenQuickSuggestion",
    }
}

string_enum! {
    /// `'prefix' | 'subword' | 'subwordSmart'`.
    pub enum SuggestPreviewMode {
        Prefix => "prefix",
        Subword => "subword",
        SubwordSmart => "subwordSmart",
    }
}

/// `InternalSuggestOptions` (`Readonly<Required<ISuggestOptions>>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalSuggestOptions {
    /// Sobrescreve o fim da palavra ao aceitar.
    pub insert_mode: SuggestInsertMode,
    /// Casamento tolerante a erros de digitação.
    pub filter_graceful: bool,
    /// Um snippet ativo impede as sugestões rápidas.
    pub snippets_prevent_quick_suggestions: bool,
    /// Favorece palavras próximas do cursor.
    pub locality_bonus: bool,
    /// Compartilha as seleções lembradas entre janelas.
    pub share_suggest_selections: bool,
    /// Quando seleciona uma sugestão ao acionar por sugestão rápida ou por caractere gatilho.
    pub selection_mode: SuggestSelectionMode,
    pub show_icons: bool,
    pub show_status_bar: bool,
    /// Pré-visualiza o resultado da sugestão.
    pub preview: bool,
    pub preview_mode: SuggestPreviewMode,
    pub show_inline_details: bool,
    /// Alarga a largura preferida do widget para caber o detalhe em linha (interno).
    pub fit_width_to_details: bool,
    pub show_methods: bool,
    pub show_functions: bool,
    pub show_constructors: bool,
    pub show_deprecated: bool,
    pub match_on_word_start_only: bool,
    pub show_fields: bool,
    pub show_variables: bool,
    pub show_classes: bool,
    pub show_structs: bool,
    pub show_interfaces: bool,
    pub show_modules: bool,
    pub show_properties: bool,
    pub show_events: bool,
    pub show_operators: bool,
    pub show_units: bool,
    pub show_values: bool,
    pub show_constants: bool,
    pub show_enums: bool,
    pub show_enum_members: bool,
    pub show_keywords: bool,
    pub show_words: bool,
    pub show_colors: bool,
    pub show_files: bool,
    pub show_references: bool,
    pub show_folders: bool,
    pub show_type_parameters: bool,
    pub show_snippets: bool,
    pub show_users: bool,
    pub show_issues: bool,
}

/// `{ type: 'boolean', default, <text_key>: text }`.
fn bool_entry(default_value: bool, text_key: &'static str, text: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string("boolean")),
        ("default", Dynamic::Bool(default_value)),
        (text_key, Dynamic::string(text)),
    ])
}

/// `{ type: 'string', enum, enumDescriptions, default, <text_key>: text }`.
fn enum_entry(allowed: &[&str], descriptions: &[&str], default_value: &str, text_key: &'static str, text: &str) -> Dynamic {
    Dynamic::object([
        ("type", Dynamic::string("string")),
        ("enum", strings_array(allowed)),
        ("enumDescriptions", strings_array(descriptions)),
        ("default", Dynamic::string(default_value)),
        (text_key, Dynamic::string(text)),
    ])
}

/// As entradas `editor.suggest.show*` e `matchOnWordStartOnly` do esquema, na ordem do original: o nome
/// do campo e o `markdownDescription`. Todas com `default: true`.
const SHOW_ENTRIES: &[(&str, &str)] = &[
    ("showMethods", "When enabled IntelliSense shows `method`-suggestions."),
    ("showFunctions", "When enabled IntelliSense shows `function`-suggestions."),
    ("showConstructors", "When enabled IntelliSense shows `constructor`-suggestions."),
    ("showDeprecated", "When enabled IntelliSense shows `deprecated`-suggestions."),
    (
        "matchOnWordStartOnly",
        "When enabled IntelliSense filtering requires that the first character matches on a word start. For example, `c` on `Console` or `WebContext` but _not_ on `description`. When disabled IntelliSense will show more results but still sorts them by match quality.",
    ),
    ("showFields", "When enabled IntelliSense shows `field`-suggestions."),
    ("showVariables", "When enabled IntelliSense shows `variable`-suggestions."),
    ("showClasses", "When enabled IntelliSense shows `class`-suggestions."),
    ("showStructs", "When enabled IntelliSense shows `struct`-suggestions."),
    ("showInterfaces", "When enabled IntelliSense shows `interface`-suggestions."),
    ("showModules", "When enabled IntelliSense shows `module`-suggestions."),
    ("showProperties", "When enabled IntelliSense shows `property`-suggestions."),
    ("showEvents", "When enabled IntelliSense shows `event`-suggestions."),
    ("showOperators", "When enabled IntelliSense shows `operator`-suggestions."),
    ("showUnits", "When enabled IntelliSense shows `unit`-suggestions."),
    ("showValues", "When enabled IntelliSense shows `value`-suggestions."),
    ("showConstants", "When enabled IntelliSense shows `constant`-suggestions."),
    ("showEnums", "When enabled IntelliSense shows `enum`-suggestions."),
    ("showEnumMembers", "When enabled IntelliSense shows `enumMember`-suggestions."),
    ("showKeywords", "When enabled IntelliSense shows `keyword`-suggestions."),
    ("showWords", "When enabled IntelliSense shows `text`-suggestions."),
    ("showColors", "When enabled IntelliSense shows `color`-suggestions."),
    ("showFiles", "When enabled IntelliSense shows `file`-suggestions."),
    ("showReferences", "When enabled IntelliSense shows `reference`-suggestions."),
    ("showCustomcolors", "When enabled IntelliSense shows `customcolor`-suggestions."),
    ("showFolders", "When enabled IntelliSense shows `folder`-suggestions."),
    ("showTypeParameters", "When enabled IntelliSense shows `typeParameter`-suggestions."),
    ("showSnippets", "When enabled IntelliSense shows `snippet`-suggestions."),
    ("showUsers", "When enabled IntelliSense shows `user`-suggestions."),
    ("showIssues", "When enabled IntelliSense shows `issues`-suggestions."),
];

/// `EditorSuggest`.
pub struct EditorSuggest {
    default_value: InternalSuggestOptions,
    schema: Dynamic,
}

impl EditorSuggest {
    pub fn new() -> Self {
        let defaults = InternalSuggestOptions {
            insert_mode: SuggestInsertMode::Insert,
            filter_graceful: true,
            snippets_prevent_quick_suggestions: false,
            locality_bonus: false,
            share_suggest_selections: false,
            selection_mode: SuggestSelectionMode::Always,
            show_icons: true,
            show_status_bar: false,
            preview: false,
            preview_mode: SuggestPreviewMode::SubwordSmart,
            show_inline_details: true,
            fit_width_to_details: false,
            show_methods: true,
            show_functions: true,
            show_constructors: true,
            show_deprecated: true,
            match_on_word_start_only: true,
            show_fields: true,
            show_variables: true,
            show_classes: true,
            show_structs: true,
            show_interfaces: true,
            show_modules: true,
            show_properties: true,
            show_events: true,
            show_operators: true,
            show_units: true,
            show_values: true,
            show_constants: true,
            show_enums: true,
            show_enum_members: true,
            show_keywords: true,
            show_words: true,
            show_colors: true,
            show_files: true,
            show_references: true,
            show_folders: true,
            show_type_parameters: true,
            show_snippets: true,
            show_users: true,
            show_issues: true,
        };

        let mut entries: Vec<(String, Dynamic)> = vec![
            (
                "editor.suggest.insertMode".to_string(),
                enum_entry(
                    SuggestInsertMode::ALLOWED,
                    &[
                        "Insert suggestion without overwriting text right of the cursor.",
                        "Insert suggestion and overwrite text right of the cursor.",
                    ],
                    defaults.insert_mode.as_str(),
                    "description",
                    "Controls whether words are overwritten when accepting completions. Note that this depends on extensions opting into this feature.",
                ),
            ),
            (
                "editor.suggest.filterGraceful".to_string(),
                bool_entry(defaults.filter_graceful, "description", "Controls whether filtering and sorting suggestions accounts for small typos."),
            ),
            (
                "editor.suggest.localityBonus".to_string(),
                bool_entry(defaults.locality_bonus, "description", "Controls whether sorting favors words that appear close to the cursor."),
            ),
            (
                "editor.suggest.shareSuggestSelections".to_string(),
                bool_entry(
                    defaults.share_suggest_selections,
                    "markdownDescription",
                    "Controls whether remembered suggestion selections are shared between multiple workspaces and windows (needs `#editor.suggestSelection#`).",
                ),
            ),
            (
                "editor.suggest.selectionMode".to_string(),
                enum_entry(
                    SuggestSelectionMode::ALLOWED,
                    &[
                        "Always select a suggestion when automatically triggering IntelliSense.",
                        "Never select a suggestion when automatically triggering IntelliSense.",
                        "Select a suggestion only when triggering IntelliSense from a trigger character.",
                        "Select a suggestion only when triggering IntelliSense as you type.",
                    ],
                    defaults.selection_mode.as_str(),
                    "markdownDescription",
                    "Controls whether a suggestion is selected when the widget shows. Note that this only applies to automatically triggered suggestions (`#editor.quickSuggestions#` and `#editor.suggestOnTriggerCharacters#`) and that a suggestion is always selected when explicitly invoked, e.g via `Ctrl+Space`.",
                ),
            ),
            (
                "editor.suggest.snippetsPreventQuickSuggestions".to_string(),
                bool_entry(
                    defaults.snippets_prevent_quick_suggestions,
                    "description",
                    "Controls whether an active snippet prevents quick suggestions.",
                ),
            ),
            (
                "editor.suggest.showIcons".to_string(),
                bool_entry(defaults.show_icons, "description", "Controls whether to show or hide icons in suggestions."),
            ),
            (
                "editor.suggest.showStatusBar".to_string(),
                bool_entry(
                    defaults.show_status_bar,
                    "description",
                    "Controls the visibility of the status bar at the bottom of the suggest widget.",
                ),
            ),
            (
                "editor.suggest.preview".to_string(),
                bool_entry(defaults.preview, "description", "Controls whether to preview the suggestion outcome in the editor."),
            ),
            (
                "editor.suggest.showInlineDetails".to_string(),
                bool_entry(
                    defaults.show_inline_details,
                    "description",
                    "Controls whether suggest details show inline with the label or only in the details widget.",
                ),
            ),
            (
                "editor.suggest.filteredTypes".to_string(),
                Dynamic::object([
                    ("type", Dynamic::string("object")),
                    (
                        "deprecationMessage",
                        Dynamic::string("This setting is deprecated, please use separate settings like 'editor.suggest.showKeywords' or 'editor.suggest.showSnippets' instead."),
                    ),
                ]),
            ),
        ];
        for (field, text) in SHOW_ENTRIES {
            entries.push((format!("editor.suggest.{field}"), bool_entry(true, "markdownDescription", text)));
        }
        let schema = Dynamic::object(entries.iter().map(|(key, value)| (key.as_str(), value.clone())));
        Self { default_value: defaults, schema }
    }
}

impl Default for EditorSuggest {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorSuggest {
    type Value = InternalSuggestOptions;

    fn id(&self) -> EditorOption {
        EditorOption::Suggest
    }

    fn name(&self) -> &str {
        "suggest"
    }

    fn default_value(&self) -> InternalSuggestOptions {
        self.default_value.clone()
    }

    fn schema(&self) -> Option<&Dynamic> {
        Some(&self.schema)
    }

    fn validate(&self, input: &Dynamic) -> InternalSuggestOptions {
        if !input.is_truthy() || input.type_of() != "object" {
            return self.default_value.clone();
        }
        let defaults = &self.default_value;
        let flag = |key: &str, default_value: bool| boolean(&input.get_str(key), default_value);
        InternalSuggestOptions {
            insert_mode: SuggestInsertMode::validate(&input.get_str("insertMode"), defaults.insert_mode),
            filter_graceful: flag("filterGraceful", defaults.filter_graceful),
            // O original usa o padrão de `filterGraceful` aqui (e não o do próprio campo).
            snippets_prevent_quick_suggestions: flag("snippetsPreventQuickSuggestions", defaults.filter_graceful),
            locality_bonus: flag("localityBonus", defaults.locality_bonus),
            share_suggest_selections: flag("shareSuggestSelections", defaults.share_suggest_selections),
            selection_mode: SuggestSelectionMode::validate(&input.get_str("selectionMode"), defaults.selection_mode),
            show_icons: flag("showIcons", defaults.show_icons),
            show_status_bar: flag("showStatusBar", defaults.show_status_bar),
            preview: flag("preview", defaults.preview),
            preview_mode: SuggestPreviewMode::validate(&input.get_str("previewMode"), defaults.preview_mode),
            show_inline_details: flag("showInlineDetails", defaults.show_inline_details),
            fit_width_to_details: flag("fitWidthToDetails", defaults.fit_width_to_details),
            show_methods: flag("showMethods", defaults.show_methods),
            show_functions: flag("showFunctions", defaults.show_functions),
            show_constructors: flag("showConstructors", defaults.show_constructors),
            show_deprecated: flag("showDeprecated", defaults.show_deprecated),
            match_on_word_start_only: flag("matchOnWordStartOnly", defaults.match_on_word_start_only),
            show_fields: flag("showFields", defaults.show_fields),
            show_variables: flag("showVariables", defaults.show_variables),
            show_classes: flag("showClasses", defaults.show_classes),
            show_structs: flag("showStructs", defaults.show_structs),
            show_interfaces: flag("showInterfaces", defaults.show_interfaces),
            show_modules: flag("showModules", defaults.show_modules),
            show_properties: flag("showProperties", defaults.show_properties),
            show_events: flag("showEvents", defaults.show_events),
            show_operators: flag("showOperators", defaults.show_operators),
            show_units: flag("showUnits", defaults.show_units),
            show_values: flag("showValues", defaults.show_values),
            show_constants: flag("showConstants", defaults.show_constants),
            show_enums: flag("showEnums", defaults.show_enums),
            show_enum_members: flag("showEnumMembers", defaults.show_enum_members),
            show_keywords: flag("showKeywords", defaults.show_keywords),
            show_words: flag("showWords", defaults.show_words),
            show_colors: flag("showColors", defaults.show_colors),
            show_files: flag("showFiles", defaults.show_files),
            show_references: flag("showReferences", defaults.show_references),
            show_folders: flag("showFolders", defaults.show_folders),
            show_type_parameters: flag("showTypeParameters", defaults.show_type_parameters),
            show_snippets: flag("showSnippets", defaults.show_snippets),
            show_users: flag("showUsers", defaults.show_users),
            show_issues: flag("showIssues", defaults.show_issues),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Dynamic {
        Dynamic::string(s)
    }

    fn validate(entries: Vec<(&str, Dynamic)>) -> InternalSuggestOptions {
        EditorSuggest::new().validate(&Dynamic::object(entries))
    }

    #[test]
    fn defaults_match_the_original() {
        let defaults = EditorSuggest::new().default_value();
        assert_eq!(defaults.insert_mode, SuggestInsertMode::Insert);
        assert_eq!(defaults.selection_mode, SuggestSelectionMode::Always);
        assert_eq!(defaults.preview_mode, SuggestPreviewMode::SubwordSmart);
        assert!(defaults.filter_graceful && defaults.show_icons && defaults.show_inline_details);
        assert!(!defaults.snippets_prevent_quick_suggestions && !defaults.locality_bonus && !defaults.preview);
        assert!(!defaults.share_suggest_selections && !defaults.show_status_bar && !defaults.fit_width_to_details);
        assert!(defaults.show_methods && defaults.show_issues && defaults.match_on_word_start_only);
    }

    #[test]
    fn invalid_inputs_give_the_default() {
        let option = EditorSuggest::new();
        for input in [Dynamic::Undefined, Dynamic::Null, Dynamic::Bool(true), Dynamic::Number(1.0), text("on")] {
            assert_eq!(option.validate(&input), option.default_value());
        }
        // Um objeto vazio não volta ao padrão: o original lê `snippetsPreventQuickSuggestions` com o padrão de
        // `filterGraceful` (`true`), não com o do próprio campo (`false`).
        let from_empty_object = InternalSuggestOptions { snippets_prevent_quick_suggestions: true, ..option.default_value() };
        assert_eq!(validate(vec![]), from_empty_object);
    }

    #[test]
    fn enums_accept_only_the_listed_strings() {
        let value = validate(vec![
            ("insertMode", text("replace")),
            ("selectionMode", text("whenQuickSuggestion")),
            ("previewMode", text("prefix")),
        ]);
        assert_eq!(value.insert_mode, SuggestInsertMode::Replace);
        assert_eq!(value.selection_mode, SuggestSelectionMode::WhenQuickSuggestion);
        assert_eq!(value.preview_mode, SuggestPreviewMode::Prefix);
        let invalid = validate(vec![
            ("insertMode", text("overwrite")),
            ("selectionMode", Dynamic::Number(1.0)),
            ("previewMode", text("Prefix")),
        ]);
        // Os enums inválidos caem no padrão; `snippetsPreventQuickSuggestions` segue o padrão de `filterGraceful`.
        assert_eq!(invalid, InternalSuggestOptions { snippets_prevent_quick_suggestions: true, ..EditorSuggest::new().default_value() });
    }

    #[test]
    fn booleans_follow_the_boolean_helper() {
        let value = validate(vec![
            ("filterGraceful", text("false")),
            ("showIcons", Dynamic::Number(0.0)),
            ("preview", text("x")),
            ("showIssues", Dynamic::Bool(false)),
            ("fitWidthToDetails", Dynamic::Bool(true)),
        ]);
        assert!(!value.filter_graceful && !value.show_icons && !value.show_issues);
        assert!(value.preview && value.fit_width_to_details);
        assert!(value.show_methods);
    }

    #[test]
    fn snippets_prevent_quick_suggestions_falls_back_to_the_filter_graceful_default() {
        // Quirk fiel do original: o padrão do campo ausente é `true` (o de `filterGraceful`).
        assert!(validate(vec![("showIcons", Dynamic::Bool(true))]).snippets_prevent_quick_suggestions);
        assert!(!validate(vec![("snippetsPreventQuickSuggestions", Dynamic::Bool(false))]).snippets_prevent_quick_suggestions);
        assert!(validate(vec![("snippetsPreventQuickSuggestions", Dynamic::Bool(true))]).snippets_prevent_quick_suggestions);
    }

    #[test]
    fn schema_keeps_the_original_keys_and_order() {
        let option = EditorSuggest::new();
        let schema = option.schema().expect("sem esquema");
        let keys: Vec<String> = schema.own_keys().iter().map(|key| key.to_string()).collect();
        assert_eq!(keys.len(), 41);
        assert_eq!(
            keys[..12].to_vec(),
            vec![
                "editor.suggest.insertMode",
                "editor.suggest.filterGraceful",
                "editor.suggest.localityBonus",
                "editor.suggest.shareSuggestSelections",
                "editor.suggest.selectionMode",
                "editor.suggest.snippetsPreventQuickSuggestions",
                "editor.suggest.showIcons",
                "editor.suggest.showStatusBar",
                "editor.suggest.preview",
                "editor.suggest.showInlineDetails",
                "editor.suggest.filteredTypes",
                "editor.suggest.showMethods",
            ]
        );
        assert_eq!(keys[15], "editor.suggest.matchOnWordStartOnly");
        assert_eq!(keys[40], "editor.suggest.showIssues");
        assert!(keys.contains(&"editor.suggest.showCustomcolors".to_string()));
        assert!(!keys.contains(&"editor.suggest.previewMode".to_string()));
        assert!(!keys.contains(&"editor.suggest.fitWidthToDetails".to_string()));

        let insert_mode = schema.get_str("editor.suggest.insertMode");
        assert_eq!(insert_mode.own_keys(), vec!["type", "enum", "enumDescriptions", "default", "description"]);
        assert_eq!(insert_mode.get_str("default"), text("insert"));
        let selection_mode = schema.get_str("editor.suggest.selectionMode");
        assert_eq!(selection_mode.own_keys(), vec!["type", "enum", "enumDescriptions", "default", "markdownDescription"]);
        assert_eq!(selection_mode.get_str("enum").get_str("2"), text("whenTriggerCharacter"));
        let filtered = schema.get_str("editor.suggest.filteredTypes");
        assert_eq!(filtered.own_keys(), vec!["type", "deprecationMessage"]);
        let methods = schema.get_str("editor.suggest.showMethods");
        assert_eq!(methods.own_keys(), vec!["type", "default", "markdownDescription"]);
        assert_eq!(methods.get_str("default"), Dynamic::Bool(true));
        let graceful = schema.get_str("editor.suggest.filterGraceful");
        assert_eq!(graceful.own_keys(), vec!["type", "default", "description"]);
    }
}
