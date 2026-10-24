//! Porte de `src/vs/editor/browser/config/migrateOptions.ts`: a compatibilidade com opções antigas. Cada
//! migração lê uma chave (com pontos para os objetos aninhados), e escreve o valor novo (ou `undefined`, que
//! mantém a chave com valor indefinido, como o original).
//!
//! Divergências:
//! - A lista `EditorSettingMigration.items` (estática do original, preenchida ao carregar o módulo) é construída
//!   uma vez por thread, na primeira chamada de [`migrate_options`].
//! - As opções são um objeto [`Dynamic`] (o `IEditorOptions`). Escrever em algo que não é objeto lança `TypeError`,
//!   como o `"use strict"` dos módulos do original (um valor primitivo, `null` e `undefined` não aceitam
//!   propriedades; o caso de escrever num array, que o JavaScript aceitaria como propriedade extra, também lança).
//! - Os valores novos das migrações simples (`{ enabled: true }`, por exemplo) são criados uma vez só e
//!   compartilhados entre todas as aplicações, como o literal do original.

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};

/// `ISettingsReader`.
type SettingsReader<'a> = &'a dyn Fn(&str) -> Dynamic;

/// `ISettingsWriter`.
type SettingsWriter<'a> = &'a dyn Fn(&str, Dynamic);

/// O `migrate` de `EditorSettingMigration`.
type Migrate = Box<dyn Fn(&Dynamic, SettingsReader, SettingsWriter)>;

/// `EditorSettingMigration`.
pub struct EditorSettingMigration {
    key: String,
    migrate: Migrate,
}

impl EditorSettingMigration {
    pub fn new(key: &str, migrate: impl Fn(&Dynamic, SettingsReader, SettingsWriter) + 'static) -> Self {
        Self { key: key.to_owned(), migrate: Box::new(migrate) }
    }

    pub fn apply(&self, options: &Dynamic) {
        let value = Self::read(options, &self.key);
        let read = |key: &str| Self::read(options, key);
        let write = |key: &str, value: Dynamic| Self::write(options, key, value);
        (self.migrate)(&value, &read, &write);
    }

    /// `_read`.
    fn read(source: &Dynamic, key: &str) -> Dynamic {
        if source.is_nullish() {
            return Dynamic::Undefined;
        }
        match key.split_once('.') {
            Some((first_segment, rest)) => Self::read(&source.get_str(first_segment), rest),
            None => source.get_str(key),
        }
    }

    /// `_write`.
    fn write(target: &Dynamic, key: &str, value: Dynamic) {
        let Dynamic::Object(object) = target else {
            errors::throw(Error::type_error(format!("Cannot create property '{key}' on a value that is not an object")));
        };
        match key.split_once('.') {
            Some((first_segment, rest)) => {
                let first_segment = JsString::from(first_segment);
                // `target[first] = target[first] || {}`.
                let current = object.get(&first_segment).unwrap_or(Dynamic::Undefined);
                let nested = if current.is_truthy() { current } else { Dynamic::object::<&str>([]) };
                object.set(&first_segment, nested.clone());
                Self::write(&nested, rest, value);
            }
            None => object.set(&JsString::from(key), value),
        }
    }
}

/// `registerSimpleEditorSettingMigration`.
fn simple_migration(key: &str, values: Vec<(Dynamic, Dynamic)>) -> EditorSettingMigration {
    let owned_key = key.to_owned();
    EditorSettingMigration::new(key, move |value, _read, write| {
        if value.is_undefined() {
            return;
        }
        for (old_value, new_value) in &values {
            if value.strict_equals(old_value) {
                write(&owned_key, new_value.clone());
                return;
            }
        }
    })
}

fn boolean_pair(true_value: Dynamic, false_value: Dynamic) -> Vec<(Dynamic, Dynamic)> {
    vec![(Dynamic::Bool(true), true_value), (Dynamic::Bool(false), false_value)]
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn enabled(value: bool) -> Dynamic {
    Dynamic::object([("enabled", Dynamic::Bool(value))])
}

fn is_type(value: &Dynamic, type_name: &str) -> bool {
    value.type_of() == type_name
}

/// `suggestFilteredTypesMapping`.
const SUGGEST_FILTERED_TYPES_MAPPING: [(&str, &str); 26] = [
    ("method", "showMethods"),
    ("function", "showFunctions"),
    ("constructor", "showConstructors"),
    ("deprecated", "showDeprecated"),
    ("field", "showFields"),
    ("variable", "showVariables"),
    ("class", "showClasses"),
    ("struct", "showStructs"),
    ("interface", "showInterfaces"),
    ("module", "showModules"),
    ("property", "showProperties"),
    ("event", "showEvents"),
    ("operator", "showOperators"),
    ("unit", "showUnits"),
    ("value", "showValues"),
    ("constant", "showConstants"),
    ("enum", "showEnums"),
    ("enumMember", "showEnumMembers"),
    ("keyword", "showKeywords"),
    ("text", "showWords"),
    ("color", "showColors"),
    ("file", "showFiles"),
    ("reference", "showReferences"),
    ("folder", "showFolders"),
    ("typeParameter", "showTypeParameters"),
    ("snippet", "showSnippets"),
];

/// Escreve `new_key` com o valor de `old_key` movido, se `new_key` ainda não existe (o padrão de várias migrações).
fn move_if_unset(read: SettingsReader, write: SettingsWriter, new_key: &str, value: Dynamic) {
    if read(new_key).is_undefined() {
        write(new_key, value);
    }
}

/// Todas as migrações, na ordem de registro do original.
fn build_migrations() -> Vec<EditorSettingMigration> {
    let mut items = vec![
        simple_migration("wordWrap", boolean_pair(text("on"), text("off"))),
        simple_migration("lineNumbers", boolean_pair(text("on"), text("off"))),
        simple_migration("cursorBlinking", vec![(text("visible"), text("solid"))]),
        simple_migration("renderWhitespace", boolean_pair(text("boundary"), text("none"))),
        simple_migration("renderLineHighlight", boolean_pair(text("line"), text("none"))),
        simple_migration("acceptSuggestionOnEnter", boolean_pair(text("on"), text("off"))),
        simple_migration("tabCompletion", boolean_pair(text("onlySnippets"), text("off"))),
        simple_migration("hover", boolean_pair(enabled(true), enabled(false))),
        simple_migration("parameterHints", boolean_pair(enabled(true), enabled(false))),
        simple_migration("autoIndent", boolean_pair(text("full"), text("advanced"))),
        simple_migration("matchBrackets", boolean_pair(text("always"), text("never"))),
        simple_migration("renderFinalNewline", boolean_pair(text("on"), text("off"))),
        simple_migration("cursorSmoothCaretAnimation", boolean_pair(text("on"), text("off"))),
        simple_migration("occurrencesHighlight", boolean_pair(text("singleFile"), text("off"))),
        simple_migration("wordBasedSuggestions", boolean_pair(text("matchingDocuments"), text("off"))),
        simple_migration("defaultColorDecorators", boolean_pair(text("auto"), text("never"))),
        simple_migration("minimap.autohide", boolean_pair(text("mouseover"), text("none"))),
    ];
    // O `autoIndent` do original lista `[false, 'advanced'], [true, 'full']`: a ordem dos pares não muda o
    // resultado, porque `true` e `false` são excludentes.

    items.push(EditorSettingMigration::new("autoClosingBrackets", |value, read, write| {
        if matches!(value, Dynamic::Bool(false)) {
            write("autoClosingBrackets", text("never"));
            if read("autoClosingQuotes").is_undefined() {
                write("autoClosingQuotes", text("never"));
            }
            if read("autoSurround").is_undefined() {
                write("autoSurround", text("never"));
            }
        }
    }));

    items.push(EditorSettingMigration::new("renderIndentGuides", |value, read, write| {
        if !value.is_undefined() {
            write("renderIndentGuides", Dynamic::Undefined);
            move_if_unset(read, write, "guides.indentation", Dynamic::Bool(value.is_truthy()));
        }
    }));

    items.push(EditorSettingMigration::new("highlightActiveIndentGuide", |value, read, write| {
        if !value.is_undefined() {
            write("highlightActiveIndentGuide", Dynamic::Undefined);
            move_if_unset(read, write, "guides.highlightActiveIndentation", Dynamic::Bool(value.is_truthy()));
        }
    }));

    items.push(EditorSettingMigration::new("suggest.filteredTypes", |value, read, write| {
        if value.is_truthy() && is_type(value, "object") {
            for (type_name, new_key) in SUGGEST_FILTERED_TYPES_MAPPING {
                if matches!(value.get_str(type_name), Dynamic::Bool(false)) {
                    move_if_unset(read, write, &format!("suggest.{new_key}"), Dynamic::Bool(false));
                }
            }
            write("suggest.filteredTypes", Dynamic::Undefined);
        }
    }));

    items.push(EditorSettingMigration::new("quickSuggestions", |input, _read, write| {
        if let Dynamic::Bool(input) = input {
            let value = text(if *input { "on" } else { "off" });
            write(
                "quickSuggestions",
                Dynamic::object([("comments", value.clone()), ("strings", value.clone()), ("other", value)]),
            );
        }
    }));

    // Sticky Scroll

    items.push(EditorSettingMigration::new("experimental.stickyScroll.enabled", |value, read, write| {
        if is_type(value, "boolean") {
            write("experimental.stickyScroll.enabled", Dynamic::Undefined);
            move_if_unset(read, write, "stickyScroll.enabled", value.clone());
        }
    }));

    items.push(EditorSettingMigration::new("experimental.stickyScroll.maxLineCount", |value, read, write| {
        if is_type(value, "number") {
            write("experimental.stickyScroll.maxLineCount", Dynamic::Undefined);
            move_if_unset(read, write, "stickyScroll.maxLineCount", value.clone());
        }
    }));

    // Edit Context

    items.push(EditorSettingMigration::new("editor.experimentalEditContextEnabled", |value, read, write| {
        if is_type(value, "boolean") {
            write("editor.experimentalEditContextEnabled", Dynamic::Undefined);
            move_if_unset(read, write, "editor.editContext", value.clone());
        }
    }));

    // Code Actions on Save

    items.push(EditorSettingMigration::new("codeActionsOnSave", |value, _read, write| {
        if value.is_truthy() && is_type(value, "object") {
            let mut to_be_modified = false;
            let mut new_entries: Vec<(JsString, Dynamic)> = Vec::new();
            for (key, entry) in value.own_entries() {
                if let Dynamic::Bool(flag) = entry {
                    to_be_modified = true;
                    new_entries.push((key, text(if flag { "explicit" } else { "never" })));
                } else {
                    new_entries.push((key, entry));
                }
            }
            if to_be_modified {
                write("codeActionsOnSave", Dynamic::object(new_entries));
            }
        }
    }));

    // Migrate Quick Fix Settings

    items.push(EditorSettingMigration::new("codeActionWidget.includeNearbyQuickfixes", |value, read, write| {
        if is_type(value, "boolean") {
            write("codeActionWidget.includeNearbyQuickfixes", Dynamic::Undefined);
            move_if_unset(read, write, "codeActionWidget.includeNearbyQuickFixes", value.clone());
        }
    }));

    // Migrate the lightbulb settings

    items.push(EditorSettingMigration::new("lightbulb.enabled", |value, _read, write| {
        if let Dynamic::Bool(enabled) = value {
            write("lightbulb.enabled", if *enabled { Dynamic::Undefined } else { text("off") });
        }
    }));

    // NES Code Shifting

    items.push(EditorSettingMigration::new("inlineSuggest.edits.codeShifting", |value, _read, write| {
        if let Dynamic::Bool(code_shifting) = value {
            write("inlineSuggest.edits.codeShifting", Dynamic::Undefined);
            write("inlineSuggest.edits.allowCodeShifting", text(if *code_shifting { "always" } else { "never" }));
        }
    }));

    // Migrate Hover

    items.push(EditorSettingMigration::new("hover.enabled", |value, _read, write| {
        if let Dynamic::Bool(hover_enabled) = value {
            write("hover.enabled", text(if *hover_enabled { "on" } else { "off" }));
        }
    }));

    items
}

thread_local! {
    /// `EditorSettingMigration.items`.
    static ITEMS: Vec<EditorSettingMigration> = build_migrations();
}

/// `migrateOptions(options)`: compatibilidade com as opções antigas. Modifica `options`.
pub fn migrate_options(options: &Dynamic) {
    ITEMS.with(|items| items.iter().for_each(|migration| migration.apply(options)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(value: f64) -> Dynamic {
        Dynamic::Number(value)
    }

    fn migrated(options: Dynamic) -> Dynamic {
        migrate_options(&options);
        options
    }

    #[test]
    fn simple_values_are_translated() {
        let options = migrated(Dynamic::object([
            ("wordWrap", Dynamic::Bool(true)),
            ("lineNumbers", Dynamic::Bool(false)),
            ("cursorBlinking", text("visible")),
            ("autoIndent", Dynamic::Bool(false)),
        ]));
        assert_eq!(options.get_str("wordWrap"), text("on"));
        assert_eq!(options.get_str("lineNumbers"), text("off"));
        assert_eq!(options.get_str("cursorBlinking"), text("solid"));
        assert_eq!(options.get_str("autoIndent"), text("advanced"));
    }

    #[test]
    fn untouched_options_stay_as_they_were() {
        let options = migrated(Dynamic::object([("wordWrap", text("bounded")), ("fontSize", number(14.0))]));
        assert_eq!(options.get_str("wordWrap"), text("bounded"));
        assert_eq!(options.get_str("fontSize"), number(14.0));
        assert!(options.get_str("lineNumbers").is_undefined());
    }

    #[test]
    fn nested_keys_are_read_and_written() {
        let options = migrated(Dynamic::object([(
            "minimap",
            Dynamic::object([("autohide", Dynamic::Bool(true))]),
        )]));
        assert_eq!(options.get_str("minimap").get_str("autohide"), text("mouseover"));

        let options = migrated(Dynamic::object([("renderIndentGuides", Dynamic::Bool(true))]));
        assert!(options.get_str("renderIndentGuides").is_undefined());
        assert_eq!(options.get_str("guides").get_str("indentation"), Dynamic::Bool(true));

        // A chave nova já existente não é sobrescrita.
        let options = migrated(Dynamic::object([
            ("renderIndentGuides", Dynamic::Bool(true)),
            ("guides", Dynamic::object([("indentation", Dynamic::Bool(false))])),
        ]));
        assert_eq!(options.get_str("guides").get_str("indentation"), Dynamic::Bool(false));
    }

    #[test]
    fn auto_closing_brackets_false_turns_off_the_related_options() {
        let options = migrated(Dynamic::object([("autoClosingBrackets", Dynamic::Bool(false))]));
        assert_eq!(options.get_str("autoClosingBrackets"), text("never"));
        assert_eq!(options.get_str("autoClosingQuotes"), text("never"));
        assert_eq!(options.get_str("autoSurround"), text("never"));

        let options = migrated(Dynamic::object([
            ("autoClosingBrackets", Dynamic::Bool(false)),
            ("autoClosingQuotes", text("always")),
        ]));
        assert_eq!(options.get_str("autoClosingQuotes"), text("always"));
    }

    #[test]
    fn quick_suggestions_boolean_becomes_a_record() {
        let options = migrated(Dynamic::object([("quickSuggestions", Dynamic::Bool(false))]));
        let quick = options.get_str("quickSuggestions");
        assert_eq!(quick.get_str("comments"), text("off"));
        assert_eq!(quick.get_str("strings"), text("off"));
        assert_eq!(quick.get_str("other"), text("off"));
    }

    #[test]
    fn suggest_filtered_types_become_show_flags() {
        let options = migrated(Dynamic::object([(
            "suggest",
            Dynamic::object([(
                "filteredTypes",
                Dynamic::object([("method", Dynamic::Bool(false)), ("class", Dynamic::Bool(true))]),
            )]),
        )]));
        let suggest = options.get_str("suggest");
        assert_eq!(suggest.get_str("showMethods"), Dynamic::Bool(false));
        assert!(suggest.get_str("showClasses").is_undefined());
        assert!(suggest.get_str("filteredTypes").is_undefined());
    }

    #[test]
    fn code_actions_on_save_booleans_become_strings() {
        let options = migrated(Dynamic::object([(
            "codeActionsOnSave",
            Dynamic::object([("source.fixAll", Dynamic::Bool(true)), ("source.organizeImports", text("never"))]),
        )]));
        let actions = options.get_str("codeActionsOnSave");
        assert_eq!(actions.get_str("source.fixAll"), text("explicit"));
        assert_eq!(actions.get_str("source.organizeImports"), text("never"));
    }

    #[test]
    fn lightbulb_and_hover_enabled_booleans() {
        let options = migrated(Dynamic::object([
            ("lightbulb", Dynamic::object([("enabled", Dynamic::Bool(false))])),
            ("hover", Dynamic::object([("enabled", Dynamic::Bool(true))])),
        ]));
        assert_eq!(options.get_str("lightbulb").get_str("enabled"), text("off"));
        assert_eq!(options.get_str("hover").get_str("enabled"), text("on"));

        let options = migrated(Dynamic::object([("lightbulb", Dynamic::object([("enabled", Dynamic::Bool(true))]))]));
        assert!(options.get_str("lightbulb").get_str("enabled").is_undefined());
    }

    #[test]
    fn hover_boolean_becomes_an_object_then_its_enabled_flag_is_migrated() {
        let options = migrated(Dynamic::object([("hover", Dynamic::Bool(true))]));
        assert_eq!(options.get_str("hover").get_str("enabled"), text("on"));
    }

    #[test]
    fn sticky_scroll_experimental_keys_move() {
        let options = migrated(Dynamic::object([(
            "experimental",
            Dynamic::object([(
                "stickyScroll",
                Dynamic::object([("enabled", Dynamic::Bool(true)), ("maxLineCount", number(7.0))]),
            )]),
        )]));
        let sticky = options.get_str("stickyScroll");
        assert_eq!(sticky.get_str("enabled"), Dynamic::Bool(true));
        assert_eq!(sticky.get_str("maxLineCount"), number(7.0));
    }
}
